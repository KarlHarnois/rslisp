use super::templates::TemplateBorrows;
use super::typed::{TypedBinder, TypedBlock, TypedBodyItem, TypedExpr, TypedExprKind, TypedLeaf};
use crate::core::BindingId;

pub(super) struct MethodReceivers<'t> {
    templates: &'t TemplateBorrows,
}

impl<'t> MethodReceivers<'t> {
    pub(super) fn new(templates: &'t TemplateBorrows) -> Self {
        Self { templates }
    }

    pub(super) fn calls_method_on(&self, body: &TypedBlock, parameter: BindingId) -> bool {
        let names = Self::names_of(body, parameter);
        body.expressions()
            .any(|expr| self.reaches_method_on(expr, &names))
    }

    fn names_of(body: &TypedBlock, parameter: BindingId) -> Vec<BindingId> {
        let mut names = vec![parameter];
        for (binder, value) in Self::lets_in(body) {
            if Self::is_one_of(value, &names) {
                names.push(binder.binding);
            }
        }
        names
    }

    fn lets_in(body: &TypedBlock) -> impl Iterator<Item = (&TypedBinder, &TypedExpr)> {
        body.items.iter().flat_map(|item| {
            let own = match item {
                TypedBodyItem::Let(binder, value) => Some((binder, value)),
                TypedBodyItem::Expr(_) => None,
            };
            own.into_iter().chain(item.expr().let_bindings())
        })
    }

    fn reaches_method_on(&self, expr: &TypedExpr, names: &[BindingId]) -> bool {
        self.receives(expr, names)
            || expr
                .children()
                .into_iter()
                .any(|child| self.reaches_method_on(child, names))
    }

    fn receives(&self, expr: &TypedExpr, names: &[BindingId]) -> bool {
        let TypedExprKind::Call(callee, arguments) = &expr.kind else {
            return false;
        };
        arguments.iter().enumerate().any(|(position, argument)| {
            Self::is_one_of(&argument.value, names) && self.templates.receives(callee, position)
        })
    }

    fn is_one_of(expr: &TypedExpr, names: &[BindingId]) -> bool {
        matches!(expr.kind, TypedExprKind::Leaf(TypedLeaf::Local(binding)) if names.contains(&binding))
    }
}
