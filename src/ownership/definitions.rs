use std::collections::HashSet;

use crate::core::BindingId;
use crate::types::{TypedBlock, TypedBodyItem, TypedExpr, TypedExprKind, TypedLambda};

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
            .extend(lambda.params.iter().map(|param| param.binding));
        self.block(&lambda.body);
    }

    fn block(&mut self, block: &TypedBlock) {
        for item in &block.items {
            match item {
                TypedBodyItem::Let(binder, value) => {
                    self.bindings.insert(binder.binding);
                    self.expr(value);
                }
                TypedBodyItem::Expr(expr) => self.expr(expr),
            }
        }
    }

    fn expr(&mut self, expr: &TypedExpr) {
        match &expr.kind {
            TypedExprKind::Fn(lambda) => self.lambda(lambda),
            TypedExprKind::Call(callee, arguments) => {
                self.expr(callee);
                arguments
                    .iter()
                    .for_each(|argument| self.expr(&argument.value));
            }
            TypedExprKind::Block(block) => self.block(block),
            TypedExprKind::If(test, then, otherwise) => [test, then, otherwise]
                .into_iter()
                .for_each(|part| self.expr(part)),
            TypedExprKind::New(_, fields) => fields.iter().for_each(|(_, value)| self.expr(value)),
            TypedExprKind::Field(value, _) => self.expr(value),
            TypedExprKind::Literal(_)
            | TypedExprKind::Local(_)
            | TypedExprKind::Global(_)
            | TypedExprKind::Rust(_) => {}
        }
    }
}
