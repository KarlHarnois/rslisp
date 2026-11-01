use std::collections::HashSet;

use super::problems::{BorrowedStorage, DroppedValue, OwnershipProblems};
use super::slots::DeclaredSlots;
use crate::core::{BindingId, FieldLabel, PrimitiveType};
use crate::diagnostic::Diagnostic;
use crate::source::Span;
use crate::types::{
    Borrow, Lifetime, RustType, Type, TypeDisplay, TypedArgument, TypedBinder, TypedBlock,
    TypedBodyItem, TypedControl, TypedDefinition, TypedExpr, TypedExprKind, TypedLambda,
    TypedProgram, TypedStructExpr,
};

pub struct ReturnedBorrows<'p> {
    program: &'p TypedProgram,
    slots: DeclaredSlots<'p>,
    owners_of_borrowing_closures: HashSet<BindingId>,
}

impl<'p> ReturnedBorrows<'p> {
    pub fn new(program: &'p TypedProgram) -> Self {
        Self {
            program,
            slots: DeclaredSlots::new(program),
            owners_of_borrowing_closures: BorrowingClosures { program }.owners(),
        }
    }

    pub fn check_definition(&self, definition: &TypedDefinition) -> Vec<Diagnostic> {
        self.checked_functions(definition)
            .into_iter()
            .filter_map(|(lambda, owner)| self.check(lambda, owner))
            .collect()
    }

    fn checked_functions<'d>(
        &self,
        definition: &'d TypedDefinition,
    ) -> Vec<(&'d TypedLambda, ParameterOwner)> {
        let function =
            Self::function_of(definition).map(|lambda| (lambda, ParameterOwner::Function));
        let closures = self
            .closures_to_check(definition)
            .into_iter()
            .map(|closure| (closure, ParameterOwner::Closure));
        function.into_iter().chain(closures).collect()
    }

    fn function_of(definition: &TypedDefinition) -> Option<&TypedLambda> {
        match &definition.value.kind {
            TypedExprKind::Fn(lambda) => Some(lambda),
            _ => None,
        }
    }

    fn closures_to_check<'d>(&self, definition: &'d TypedDefinition) -> Vec<&'d TypedLambda> {
        if self
            .owners_of_borrowing_closures
            .contains(&definition.binding)
        {
            return LocalBorrows::closures_in(&definition.value);
        }
        Vec::new()
    }

    fn check(&self, lambda: &TypedLambda, owner: ParameterOwner) -> Option<Diagnostic> {
        let tail = LocalBorrows::tail_of(&lambda.body)?;
        let borrows = LocalBorrows::new(lambda, owner);
        let found = borrows.returned(tail)?;
        let storage = found.holders.iter().find_map(|holder| self.storage(holder));
        Some(match storage {
            Some(storage) => {
                OwnershipProblems::stored_local_borrow(found.span, &found.dropped, &storage)
            }
            None => OwnershipProblems::local_borrow(found.span, &found.dropped),
        })
    }

    fn storage(&self, holder: &BorrowHolder) -> Option<BorrowedStorage> {
        match holder {
            BorrowHolder::Field(owner, label) => self.field_storage(*owner, label),
            BorrowHolder::Payload(variant, position) => self.payload_storage(*variant, *position),
        }
    }

    fn field_storage(&self, owner: BindingId, label: &FieldLabel) -> Option<BorrowedStorage> {
        let slot = self.slots.field(owner, label)?;
        Some(BorrowedStorage::Field {
            field: slot.name.to_string(),
            owner: self.name(slot.owner),
            owned: self.owned(slot.declared)?,
            declared: slot.span,
        })
    }

    fn payload_storage(&self, variant: BindingId, position: usize) -> Option<BorrowedStorage> {
        let slot = self.slots.payload(variant, position)?;
        let owner = self.name(slot.owner);
        let owned = self.owned(slot.declared)?;
        Some(match self.slots.payload_label(variant, position) {
            Some(field) => BorrowedStorage::NamedPayload {
                field: field.to_string(),
                variant: slot.name.to_string(),
                owner,
                owned,
                declared: slot.span,
            },
            None => BorrowedStorage::Payload {
                variant: slot.name.to_string(),
                owner,
                owned,
                declared: slot.span,
            },
        })
    }

    fn owned(&self, declared: &Type) -> Option<String> {
        let owned = self.owned_type(declared)?;
        let declares_borrow = declared.holds_reference() && !owned.holds_reference();
        declares_borrow.then(|| self.display().show(&owned))
    }

    fn owned_type(&self, ty: &Type) -> Option<Type> {
        match ty {
            Type::Ref(_, viewed) | Type::Mut(_, viewed) => self.owned_view(viewed),
            Type::Named(binding, arguments) => {
                Some(Type::Named(*binding, self.owned_types(arguments)?))
            }
            other => Some(other.clone()),
        }
    }

    fn owned_view(&self, viewed: &Type) -> Option<Type> {
        match viewed {
            Type::Primitive(PrimitiveType::Str) => Some(Type::Primitive(PrimitiveType::String)),
            Type::Named(slice, elements) if self.program.rust_types.is_slice(*slice) => {
                Some(Type::Named(self.vector()?, self.owned_types(elements)?))
            }
            other => self.owned_type(other),
        }
    }

    fn owned_types(&self, types: &[Type]) -> Option<Vec<Type>> {
        types.iter().map(|ty| self.owned_type(ty)).collect()
    }

    fn vector(&self) -> Option<BindingId> {
        self.program
            .rust_types
            .binding_with_template(RustType::VECTOR_TEMPLATE)
    }

    fn name(&self, binding: BindingId) -> String {
        self.program.bindings.get(binding).name().to_string()
    }

    fn display(&self) -> TypeDisplay<'p> {
        TypeDisplay::new(&self.program.bindings, &self.program.generics)
    }
}

struct BorrowingClosures<'p> {
    program: &'p TypedProgram,
}

impl BorrowingClosures<'_> {
    fn owners(&self) -> HashSet<BindingId> {
        self.program
            .closure_types
            .iter()
            .filter(|closure| self.can_return_borrow(&closure.ty))
            .map(|closure| closure.definition)
            .collect()
    }

    fn can_return_borrow(&self, closure: &Type) -> bool {
        matches!(closure, Type::Function(_, _, result)
            if result.holds_reference() || self.names_borrowing_type(result))
    }

    fn names_borrowing_type(&self, ty: &Type) -> bool {
        matches!(ty, Type::Named(binding, _) if self.program.definition_holds_borrow(*binding))
            || ty
                .parts()
                .into_iter()
                .any(|part| self.names_borrowing_type(part))
    }
}

struct LocalBorrow<'e> {
    span: Span,
    dropped: DroppedValue,
    holders: Vec<BorrowHolder<'e>>,
}

impl<'e> LocalBorrow<'e> {
    fn new(span: Span, dropped: DroppedValue) -> Self {
        Self {
            span,
            dropped,
            holders: Vec::new(),
        }
    }

    fn held_by(mut self, holder: BorrowHolder<'e>) -> Self {
        self.holders.push(holder);
        self
    }
}

enum BorrowHolder<'e> {
    Field(BindingId, &'e FieldLabel),
    Payload(BindingId, usize),
}

#[derive(Clone, Copy)]
enum ParameterOwner {
    Function,
    Closure,
}

impl ParameterOwner {
    fn dropped_parameter(self, span: Span) -> DroppedValue {
        match self {
            Self::Function => DroppedValue::FunctionParameter(span),
            Self::Closure => DroppedValue::ClosureParameter(span),
        }
    }
}

struct LocalBorrows<'e> {
    lambda: &'e TypedLambda,
    owner: ParameterOwner,
}

impl<'e> LocalBorrows<'e> {
    fn new(lambda: &'e TypedLambda, owner: ParameterOwner) -> Self {
        Self { lambda, owner }
    }

    fn closures_in(value: &TypedExpr) -> Vec<&TypedLambda> {
        value
            .children()
            .into_iter()
            .flat_map(Self::lambdas_in)
            .collect()
    }

    fn lambdas_in(value: &TypedExpr) -> Vec<&TypedLambda> {
        let nested = value.children().into_iter().flat_map(Self::lambdas_in);
        match &value.kind {
            TypedExprKind::Fn(lambda) => std::iter::once(lambda.as_ref()).chain(nested).collect(),
            _ => nested.collect(),
        }
    }

    fn returned(&self, expr: &'e TypedExpr) -> Option<LocalBorrow<'e>> {
        match &expr.kind {
            TypedExprKind::Borrow(_, inner) => self
                .dropped_root(inner)
                .map(|dropped| LocalBorrow::new(expr.span, dropped)),
            TypedExprKind::Call(callee, arguments) if expr.ty.holds_reference() => {
                self.borrowing_call(expr.span, callee, arguments)
            }
            TypedExprKind::Struct(TypedStructExpr::New(owner, fields)) => {
                self.borrowing_field(*owner, fields)
            }
            TypedExprKind::Variant(variant, payloads) => self.borrowing_payload(*variant, payloads),
            TypedExprKind::Control(control) => self.returned_from(control),
            _ => None,
        }
    }

    fn borrowing_call(
        &self,
        span: Span,
        callee: &'e TypedExpr,
        arguments: &'e [TypedArgument],
    ) -> Option<LocalBorrow<'e>> {
        let Type::Function(_, parameters, result) = &callee.ty else {
            return None;
        };
        let result_lifetimes = WrittenLifetimes::of(result);
        let dropped: Vec<DroppedValue> = arguments
            .iter()
            .zip(parameters)
            .filter(|(_, parameter)| result_lifetimes.reached_by(parameter))
            .map(|(argument, _)| self.borrowed_by(argument))
            .collect::<Option<_>>()?;
        dropped
            .into_iter()
            .next()
            .map(|dropped| LocalBorrow::new(span, dropped))
    }

    fn borrowed_by(&self, argument: &'e TypedArgument) -> Option<DroppedValue> {
        match argument.borrow {
            Borrow::Shared | Borrow::Mutable if !argument.value.ty.holds_reference() => {
                self.dropped_root(&argument.value)
            }
            _ => self.returned(&argument.value).map(|found| found.dropped),
        }
    }

    fn borrowing_field(
        &self,
        owner: BindingId,
        fields: &'e [(FieldLabel, TypedExpr)],
    ) -> Option<LocalBorrow<'e>> {
        fields.iter().find_map(|(label, value)| {
            self.returned(value)
                .map(|found| found.held_by(BorrowHolder::Field(owner, label)))
        })
    }

    fn borrowing_payload(
        &self,
        variant: BindingId,
        payloads: &'e [TypedExpr],
    ) -> Option<LocalBorrow<'e>> {
        payloads.iter().enumerate().find_map(|(position, value)| {
            self.returned(value)
                .map(|found| found.held_by(BorrowHolder::Payload(variant, position)))
        })
    }

    fn returned_from(&self, control: &'e TypedControl) -> Option<LocalBorrow<'e>> {
        match control {
            TypedControl::Block(block) => self.returned(Self::tail_of(block)?),
            TypedControl::If(_, then, otherwise) => {
                self.returned(then).or_else(|| self.returned(otherwise))
            }
            TypedControl::Match(matched) => matched
                .arms
                .iter()
                .find_map(|arm| self.returned(Self::tail_of(&arm.body)?)),
        }
    }

    fn dropped_root(&self, place: &TypedExpr) -> Option<DroppedValue> {
        self.dropped_value(place.place_root()?)
    }

    fn dropped_value(&self, binding: BindingId) -> Option<DroppedValue> {
        match self.parameter(binding) {
            Some(parameter) if parameter.ty.is_reference() => None,
            Some(parameter) => Some(self.owner.dropped_parameter(parameter.span)),
            None => self
                .body_binder(binding)
                .map(|binder| DroppedValue::Local(binder.span)),
        }
    }

    fn parameter(&self, binding: BindingId) -> Option<&'e TypedBinder> {
        self.lambda
            .params
            .iter()
            .find(|parameter| parameter.binding == binding)
    }

    fn body_binder(&self, binding: BindingId) -> Option<&'e TypedBinder> {
        self.lambda
            .binders()
            .find(|binder| binder.binding == binding)
            .or_else(|| {
                self.lambda
                    .body
                    .expressions()
                    .find_map(|expr| Self::binder_inside(expr, binding))
            })
    }

    fn binder_inside(expr: &'e TypedExpr, binding: BindingId) -> Option<&'e TypedBinder> {
        expr.binders()
            .into_iter()
            .find(|binder| binder.binding == binding)
            .or_else(|| {
                expr.children()
                    .into_iter()
                    .find_map(|child| Self::binder_inside(child, binding))
            })
    }

    fn tail_of(block: &TypedBlock) -> Option<&TypedExpr> {
        match block.items.last()? {
            TypedBodyItem::Expr(expr) => Some(expr),
            TypedBodyItem::Let(..) => None,
        }
    }
}

struct WrittenLifetimes(Vec<BindingId>);

impl WrittenLifetimes {
    fn of(ty: &Type) -> Self {
        let mut lifetimes = Vec::new();
        Self::collect(ty, &mut lifetimes);
        Self(lifetimes)
    }

    fn collect(ty: &Type, lifetimes: &mut Vec<BindingId>) {
        if let Some(Lifetime::Written(lifetime)) = ty.reference_lifetime() {
            lifetimes.push(lifetime);
        }
        ty.parts()
            .into_iter()
            .for_each(|part| Self::collect(part, lifetimes));
    }

    fn reached_by(&self, parameter: &Type) -> bool {
        if self.0.is_empty() {
            return parameter.holds_reference();
        }
        WrittenLifetimes::of(parameter)
            .0
            .iter()
            .any(|lifetime| self.0.contains(lifetime))
    }
}
