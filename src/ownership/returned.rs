use super::problems::{BorrowedStorage, OwnershipProblems};
use super::slots::DeclaredSlots;
use crate::core::{BindingId, FieldLabel, PrimitiveType};
use crate::diagnostic::Diagnostic;
use crate::source::Span;
use crate::types::{
    Borrow, Lifetime, RustType, Type, TypeDisplay, TypedArgument, TypedBinder, TypedBlock,
    TypedBodyItem, TypedControl, TypedExpr, TypedExprKind, TypedLambda, TypedProgram,
    TypedStructExpr,
};

pub struct ReturnedBorrows<'p> {
    program: &'p TypedProgram,
    slots: DeclaredSlots<'p>,
}

impl<'p> ReturnedBorrows<'p> {
    pub fn new(program: &'p TypedProgram) -> Self {
        Self {
            program,
            slots: DeclaredSlots::new(program),
        }
    }

    pub fn check(&self, lambda: &TypedLambda) -> Option<Diagnostic> {
        let tail = LocalBorrows::tail_of(&lambda.body)?;
        let found = LocalBorrows::new(&lambda.params).returned(tail)?;
        let defined = LocalBorrows::local_span(&lambda.body, found.binding).unwrap_or(found.span);
        let storage = found.holders.iter().find_map(|holder| self.storage(holder));
        Some(match storage {
            Some(storage) => OwnershipProblems::stored_local_borrow(found.span, defined, &storage),
            None => OwnershipProblems::local_borrow(found.span, defined),
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

struct LocalBorrow<'e> {
    span: Span,
    binding: BindingId,
    holders: Vec<BorrowHolder<'e>>,
}

impl<'e> LocalBorrow<'e> {
    fn new(span: Span, binding: BindingId) -> Self {
        Self {
            span,
            binding,
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

struct LocalBorrows<'e> {
    params: &'e [TypedBinder],
}

impl<'e> LocalBorrows<'e> {
    fn new(params: &'e [TypedBinder]) -> Self {
        Self { params }
    }

    fn returned(&self, expr: &'e TypedExpr) -> Option<LocalBorrow<'e>> {
        match &expr.kind {
            TypedExprKind::Borrow(_, inner) => self
                .local_root(inner)
                .map(|binding| LocalBorrow::new(expr.span, binding)),
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
        let locals: Vec<BindingId> = arguments
            .iter()
            .zip(parameters)
            .filter(|(_, parameter)| result_lifetimes.reached_by(parameter))
            .map(|(argument, _)| self.borrowed_by(argument))
            .collect::<Option<_>>()?;
        locals
            .first()
            .map(|binding| LocalBorrow::new(span, *binding))
    }

    fn borrowed_by(&self, argument: &'e TypedArgument) -> Option<BindingId> {
        match argument.borrow {
            Borrow::Shared | Borrow::Mutable if !argument.value.ty.holds_reference() => {
                self.local_root(&argument.value)
            }
            _ => self.returned(&argument.value).map(|found| found.binding),
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

    fn local_root(&self, place: &TypedExpr) -> Option<BindingId> {
        let binding = place.place_root()?;
        self.params
            .iter()
            .all(|param| param.binding != binding)
            .then_some(binding)
    }

    fn tail_of(block: &TypedBlock) -> Option<&TypedExpr> {
        match block.items.last()? {
            TypedBodyItem::Expr(expr) => Some(expr),
            TypedBodyItem::Let(..) => None,
        }
    }

    fn local_span(block: &TypedBlock, binding: BindingId) -> Option<Span> {
        block.items.iter().find_map(|item| match item {
            TypedBodyItem::Let(binder, _) if binder.binding == binding => Some(binder.span),
            _ => None,
        })
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
