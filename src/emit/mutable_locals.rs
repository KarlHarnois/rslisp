use std::collections::HashSet;

use crate::core::BindingId;
use crate::types::{Borrow, TypedArgument, TypedExpr, TypedExprKind};

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
        argument.value.place_root()
    }
}
