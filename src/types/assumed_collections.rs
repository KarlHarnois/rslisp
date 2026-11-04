use rustc_hash::FxHashMap;

use super::typed::{TypedExpr, TypedExprKind, TypedLeaf};
use crate::core::{BindingId, Expr, ExprKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionAssumption {
    PushTarget,
    GetCollection,
}

impl CollectionAssumption {
    pub fn operation(self) -> &'static str {
        match self {
            Self::PushTarget => "push!",
            Self::GetCollection => "get",
        }
    }

    pub fn reading(self) -> &'static str {
        match self {
            Self::PushTarget => "a target of unknown type as a vector",
            Self::GetCollection => "a collection of unknown type as a vector or a slice",
        }
    }
}

#[derive(Clone, Copy)]
struct ParameterSlot {
    function: BindingId,
    position: usize,
}

#[derive(Default)]
pub struct AssumedCollections {
    parameters: FxHashMap<BindingId, ParameterSlot>,
    assumptions: FxHashMap<(BindingId, usize), CollectionAssumption>,
}

impl AssumedCollections {
    pub fn record_parameters(&mut self, function: BindingId, value: &Expr) {
        let ExprKind::Fn(lambda) = value.kind() else {
            return;
        };
        for (position, param) in lambda.params.iter().enumerate() {
            let slot = ParameterSlot { function, position };
            self.parameters.insert(param.binding, slot);
        }
    }

    pub fn assume(&mut self, target: &TypedExpr, assumption: CollectionAssumption) {
        let Some(slot) =
            Self::parameter_of(target).and_then(|binding| self.parameters.get(&binding))
        else {
            return;
        };
        self.assumptions
            .insert((slot.function, slot.position), assumption);
    }

    pub fn get(&self, function: BindingId, position: usize) -> Option<CollectionAssumption> {
        self.assumptions.get(&(function, position)).copied()
    }

    fn parameter_of(target: &TypedExpr) -> Option<BindingId> {
        match &target.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => Some(*binding),
            TypedExprKind::Borrow(_, inner) => Self::parameter_of(inner),
            _ => None,
        }
    }
}
