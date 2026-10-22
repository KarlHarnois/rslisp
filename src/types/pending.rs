use std::collections::HashMap;

use super::ty::{Type, TypeVar};
use super::typed::{Borrow, TypedExpr, TypedExprKind};
use crate::source::Span;

pub struct PendingBorrow {
    pub var: TypeVar,
    pub argument: Type,
    pub span: Span,
}

pub struct DecidedBorrows {
    decided: HashMap<usize, Borrow>,
}

impl DecidedBorrows {
    pub fn new(decided: HashMap<usize, Borrow>) -> Self {
        Self { decided }
    }

    pub fn patch(&self, expr: &mut TypedExpr) {
        let arguments = match &mut expr.kind {
            TypedExprKind::Call(_, arguments) | TypedExprKind::Recur(arguments) => Some(arguments),
            _ => None,
        };
        if let Some(arguments) = arguments {
            for argument in arguments.iter_mut() {
                if let Borrow::Pending(id) = argument.borrow {
                    argument.borrow = self.decided.get(&id).copied().unwrap_or(Borrow::None);
                }
            }
        }
        for child in expr.children_mut() {
            self.patch(child);
        }
    }
}
