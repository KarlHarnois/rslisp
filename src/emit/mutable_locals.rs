use std::collections::HashSet;

use crate::core::BindingId;
use crate::types::{Borrow, TypedArgument, TypedExpr, TypedExprKind, TypedLeaf, TypedStructExpr};

#[derive(Default)]
pub struct MutableLocals {
    bindings: HashSet<BindingId>,
}

impl MutableLocals {
    pub fn of(value: &TypedExpr) -> HashSet<BindingId> {
        let mut found = Self::default();
        found.expr(value);
        found.bindings
    }

    fn expr(&mut self, expr: &TypedExpr) {
        if let TypedExprKind::Call(_, arguments) = &expr.kind {
            self.bindings
                .extend(arguments.iter().filter_map(Self::mutably_borrowed_local));
        }
        for child in expr.children() {
            self.expr(child);
        }
    }

    fn mutably_borrowed_local(argument: &TypedArgument) -> Option<BindingId> {
        if argument.borrow != Borrow::Mutable {
            return None;
        }
        Self::place_local(&argument.value)
    }

    fn place_local(value: &TypedExpr) -> Option<BindingId> {
        match &value.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => Some(*binding),
            TypedExprKind::Struct(TypedStructExpr::Field(owner, _)) => Self::place_local(owner),
            _ => None,
        }
    }
}
