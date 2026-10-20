use std::collections::HashSet;

use crate::core::Literal;
use crate::types::{GenericId, Type, TypedBlock, TypedBodyItem, TypedExpr, TypedExprKind};

#[derive(Default)]
pub struct LiteralGenerics {
    generics: HashSet<GenericId>,
}

impl LiteralGenerics {
    pub fn of(value: &TypedExpr) -> HashSet<GenericId> {
        let mut found = Self::default();
        found.expr(value);
        found.generics
    }

    fn expr(&mut self, expr: &TypedExpr) {
        match &expr.kind {
            TypedExprKind::Literal(Literal::Number(_)) => self.note(&expr.ty),
            TypedExprKind::Fn(lambda) => self.block(&lambda.body),
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
            TypedExprKind::Literal(Literal::String(_))
            | TypedExprKind::Local(_)
            | TypedExprKind::Global(_)
            | TypedExprKind::Rust(_) => {}
        }
    }

    fn block(&mut self, block: &TypedBlock) {
        for item in &block.items {
            match item {
                TypedBodyItem::Let(_, value) | TypedBodyItem::Expr(value) => self.expr(value),
            }
        }
    }

    fn note(&mut self, literal_type: &Type) {
        if let Type::Generic(generic) = literal_type {
            self.generics.insert(*generic);
        }
    }
}
