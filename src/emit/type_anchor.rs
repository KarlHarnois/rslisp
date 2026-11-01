use super::generic_callees::GenericCallees;
use crate::core::Literal;
use crate::types::{Type, TypedExpr, TypedExprKind, TypedLeaf};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Certainty {
    Name,
    Closure,
    Expression,
    Literal,
    Unfixed,
}

pub(super) struct TypeAnchor<'a> {
    callees: &'a GenericCallees<'a>,
}

impl<'a> TypeAnchor<'a> {
    pub(super) fn new(callees: &'a GenericCallees<'a>) -> Self {
        Self { callees }
    }

    pub(super) fn among(candidates: impl Iterator<Item = (usize, Certainty)>) -> Option<usize> {
        candidates
            .min_by_key(|(_, certainty)| *certainty)
            .map(|(position, _)| position)
    }

    pub(super) fn certainties(&self, values: &[&TypedExpr]) -> Vec<Certainty> {
        values.iter().map(|value| self.certainty(value)).collect()
    }

    pub(super) fn certainty(&self, value: &TypedExpr) -> Certainty {
        if self.fixes_no_type(value) {
            return Certainty::Unfixed;
        }
        match &value.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(_) | TypedLeaf::Global(_)) => Certainty::Name,
            TypedExprKind::Fn(_) => Certainty::Closure,
            TypedExprKind::Leaf(TypedLeaf::Literal(Literal::Number(_))) => Certainty::Literal,
            _ => Certainty::Expression,
        }
    }

    fn fixes_no_type(&self, value: &TypedExpr) -> bool {
        match &value.kind {
            TypedExprKind::Leaf(TypedLeaf::Global(binding)) => {
                self.callees.positions_of(*binding).is_some()
            }
            TypedExprKind::Leaf(TypedLeaf::Local(_)) => matches!(value.ty, Type::Function(..)),
            _ => false,
        }
    }
}
