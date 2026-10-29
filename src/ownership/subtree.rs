use std::collections::HashSet;

use crate::core::BindingId;
use crate::types::{TypedExpr, TypedExprKind, TypedLeaf};

pub struct Subtree;

impl Subtree {
    pub fn of(root: &TypedExpr) -> Vec<&TypedExpr> {
        let mut expressions = vec![root];
        let mut next = 0;
        while let Some(expr) = expressions.get(next).copied() {
            expressions.extend(expr.children());
            next += 1;
        }
        expressions
    }

    pub fn first_capture(closure: &TypedExpr) -> Option<BindingId> {
        let expressions = Self::of(closure);
        let bound: HashSet<BindingId> = expressions
            .iter()
            .flat_map(|expr| expr.binders())
            .map(|binder| binder.binding)
            .collect();
        expressions.iter().find_map(|expr| match expr.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) if !bound.contains(&binding) => {
                Some(binding)
            }
            _ => None,
        })
    }
}
