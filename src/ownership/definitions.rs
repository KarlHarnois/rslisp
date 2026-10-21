use std::collections::HashSet;

use crate::core::BindingId;
use crate::types::{TypedExpr, TypedLambda};

#[derive(Default)]
pub struct LocalDefinitions {
    bindings: HashSet<BindingId>,
}

impl LocalDefinitions {
    pub fn of(lambda: &TypedLambda) -> HashSet<BindingId> {
        let mut definitions = Self::default();
        definitions.lambda(lambda);
        definitions.bindings
    }

    fn lambda(&mut self, lambda: &TypedLambda) {
        self.bindings
            .extend(lambda.binders().map(|binder| binder.binding));
        for expr in lambda.body.expressions() {
            self.expr(expr);
        }
    }

    fn expr(&mut self, expr: &TypedExpr) {
        self.bindings
            .extend(expr.binders().into_iter().map(|binder| binder.binding));
        for child in expr.children() {
            self.expr(child);
        }
    }
}
