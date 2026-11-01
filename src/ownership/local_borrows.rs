use super::problems::DroppedValue;
use crate::core::{BindingId, FieldLabel};
use crate::source::Span;
use crate::types::{
    Borrow, Lifetime, Type, TypedArgument, TypedBinder, TypedBlock, TypedBodyItem, TypedControl,
    TypedExpr, TypedExprKind, TypedLambda, TypedLeaf, TypedStructExpr,
};

pub(super) struct LocalBorrow<'e> {
    pub span: Span,
    pub dropped: DroppedValue,
    pub holders: Vec<BorrowHolder<'e>>,
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

pub(super) enum BorrowHolder<'e> {
    Field(BindingId, &'e FieldLabel),
    Payload(BindingId, usize),
}

#[derive(Clone, Copy)]
pub(super) enum ParameterOwner {
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

#[derive(Clone, Copy)]
enum BorrowRoots {
    Dropped(ParameterOwner),
    Lent(BindingId),
}

pub(super) struct LocalBorrows<'e> {
    lambda: &'e TypedLambda,
    roots: BorrowRoots,
}

impl<'e> LocalBorrows<'e> {
    pub(super) fn dropped_in(lambda: &'e TypedLambda, owner: ParameterOwner) -> Self {
        Self {
            lambda,
            roots: BorrowRoots::Dropped(owner),
        }
    }

    pub(super) fn lent_by(lambda: &'e TypedLambda, parameter: BindingId) -> Self {
        Self {
            lambda,
            roots: BorrowRoots::Lent(parameter),
        }
    }

    pub(super) fn closures_in(value: &TypedExpr) -> Vec<&TypedLambda> {
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

    pub(super) fn returned(&self, expr: &'e TypedExpr) -> Option<LocalBorrow<'e>> {
        match &expr.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => self
                .lent_reference(*binding)
                .map(|lent| LocalBorrow::new(expr.span, lent)),
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

    pub(super) fn borrowed_by(&self, argument: &'e TypedArgument) -> Option<DroppedValue> {
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

    fn dropped_root(&self, borrowed: &TypedExpr) -> Option<DroppedValue> {
        let BorrowRoots::Dropped(owner) = self.roots else {
            return None;
        };
        match borrowed.place_root() {
            Some(root) => self.dropped_value(root, owner),
            None => Self::temporary(borrowed),
        }
    }

    fn temporary(borrowed: &TypedExpr) -> Option<DroppedValue> {
        matches!(borrowed.kind, TypedExprKind::Call(..))
            .then_some(DroppedValue::Temporary(borrowed.span))
    }

    fn dropped_value(&self, binding: BindingId, owner: ParameterOwner) -> Option<DroppedValue> {
        match self.parameter(binding) {
            Some(parameter) if parameter.ty.is_reference() => None,
            Some(parameter) => Some(owner.dropped_parameter(parameter.span)),
            None => self
                .body_binder(binding)
                .map(|binder| DroppedValue::Local(binder.span)),
        }
    }

    fn lent_reference(&self, binding: BindingId) -> Option<DroppedValue> {
        match self.roots {
            BorrowRoots::Lent(parameter) if parameter == binding => self
                .parameter(binding)
                .map(|lent| DroppedValue::ClosureParameter(lent.span)),
            _ => None,
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

    pub(super) fn tail_of(block: &TypedBlock) -> Option<&TypedExpr> {
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
