use crate::core::Literal;
use crate::types::{TypedExpr, TypedExprKind, TypedLeaf};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Certainty {
    Name,
    Closure,
    Expression,
    Literal,
}

pub(super) struct TypeAnchor;

impl TypeAnchor {
    pub(super) fn among<'e>(
        candidates: impl Iterator<Item = (usize, &'e TypedExpr)>,
    ) -> Option<usize> {
        candidates
            .min_by_key(|(_, value)| Self::certainty(value))
            .map(|(position, _)| position)
    }

    fn certainty(value: &TypedExpr) -> Certainty {
        match &value.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(_) | TypedLeaf::Global(_)) => Certainty::Name,
            TypedExprKind::Fn(_) => Certainty::Closure,
            TypedExprKind::Leaf(TypedLeaf::Literal(Literal::Number(_))) => Certainty::Literal,
            _ => Certainty::Expression,
        }
    }
}
