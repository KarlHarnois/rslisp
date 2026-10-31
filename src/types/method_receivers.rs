use super::templates::TemplateBorrows;
use super::typed::{TypedBlock, TypedExpr, TypedExprKind, TypedLeaf};
use crate::core::BindingId;

pub(super) struct MethodReceivers<'t> {
    templates: &'t TemplateBorrows,
}

impl<'t> MethodReceivers<'t> {
    pub(super) fn new(templates: &'t TemplateBorrows) -> Self {
        Self { templates }
    }

    pub(super) fn calls_method_on(&self, body: &TypedBlock, local: BindingId) -> bool {
        body.expressions()
            .any(|expr| self.reaches_method_on(expr, local))
    }

    fn reaches_method_on(&self, expr: &TypedExpr, local: BindingId) -> bool {
        self.receives(expr, local)
            || expr
                .children()
                .into_iter()
                .any(|child| self.reaches_method_on(child, local))
    }

    fn receives(&self, expr: &TypedExpr, local: BindingId) -> bool {
        let TypedExprKind::Call(callee, arguments) = &expr.kind else {
            return false;
        };
        arguments.iter().enumerate().any(|(position, argument)| {
            matches!(argument.value.kind, TypedExprKind::Leaf(TypedLeaf::Local(binding)) if binding == local)
                && self.templates.receives(callee, position)
        })
    }
}
