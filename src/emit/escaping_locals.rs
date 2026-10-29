use std::collections::HashSet;

use crate::core::BindingId;
use crate::types::{TypedExpr, TypedExprKind, TypedLeaf};

pub struct EscapingLocals {
    bindings: HashSet<BindingId>,
}

impl EscapingLocals {
    pub fn of(value: &TypedExpr) -> HashSet<BindingId> {
        let mut found = Self {
            bindings: HashSet::new(),
        };
        found.expr(value);
        found.bindings
    }

    fn expr(&mut self, expr: &TypedExpr) {
        match &expr.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => {
                self.bindings.insert(*binding);
            }
            TypedExprKind::Call(callee, arguments) if Self::is_local(callee) => arguments
                .iter()
                .for_each(|argument| self.expr(&argument.value)),
            _ => expr
                .children()
                .into_iter()
                .for_each(|child| self.expr(child)),
        }
    }

    fn is_local(callee: &TypedExpr) -> bool {
        matches!(callee.kind, TypedExprKind::Leaf(TypedLeaf::Local(_)))
    }
}
