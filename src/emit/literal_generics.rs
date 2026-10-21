use std::collections::HashSet;

use crate::core::Literal;
use crate::types::{GenericId, Type, TypedExpr, TypedExprKind, TypedLeaf};

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
        if let TypedExprKind::Leaf(TypedLeaf::Literal(Literal::Number(_))) = &expr.kind {
            self.note(&expr.ty);
        }
        for child in expr.children() {
            self.expr(child);
        }
    }

    fn note(&mut self, literal_type: &Type) {
        if let Type::Generic(generic) = literal_type {
            self.generics.insert(*generic);
        }
    }
}
