use std::collections::{HashMap, HashSet};

use crate::core::BindingId;

#[derive(Clone, Default, PartialEq)]
pub(super) struct CalleeParameters {
    sharing: Vec<bool>,
    higher_ranked: Vec<bool>,
}

impl CalleeParameters {
    pub(super) fn new(sharing: Vec<bool>, higher_ranked: Vec<bool>) -> Self {
        Self {
            sharing,
            higher_ranked,
        }
    }

    pub(super) fn shares(&self, position: usize) -> bool {
        Self::marks(&self.sharing, position)
    }

    pub(super) fn is_higher_ranked(&self, position: usize) -> bool {
        Self::marks(&self.higher_ranked, position)
    }

    fn joined(&self, other: &Self) -> Self {
        Self::new(
            Self::either(&self.sharing, &other.sharing),
            Self::either(&self.higher_ranked, &other.higher_ranked),
        )
    }

    fn either(left: &[bool], right: &[bool]) -> Vec<bool> {
        (0..left.len().max(right.len()))
            .map(|position| Self::marks(left, position) || Self::marks(right, position))
            .collect()
    }

    fn marks(marked: &[bool], position: usize) -> bool {
        marked.get(position).is_some_and(|marks| *marks)
    }
}

#[derive(Default)]
pub(super) struct OpenCallees {
    orders: HashMap<BindingId, usize>,
    stack: Vec<BindingId>,
    provisional: HashMap<BindingId, CalleeParameters>,
    assumed: HashSet<BindingId>,
}

impl OpenCallees {
    pub(super) fn open(&mut self, callee: BindingId) -> usize {
        let order = self.orders.len();
        self.orders.insert(callee, order);
        self.stack.push(callee);
        order
    }

    pub(super) fn order(&self, callee: BindingId) -> Option<usize> {
        self.orders.get(&callee).copied()
    }

    pub(super) fn assume(&mut self, callee: BindingId) -> CalleeParameters {
        self.assumed.insert(callee);
        self.provisional.get(&callee).cloned().unwrap_or_default()
    }

    pub(super) fn propose(&mut self, callee: BindingId, parameters: CalleeParameters) {
        self.provisional.insert(callee, parameters);
    }

    pub(super) fn widen(&mut self, callee: BindingId, parameters: &CalleeParameters) -> bool {
        let current = self.provisional.get(&callee).cloned().unwrap_or_default();
        let widened = current.joined(parameters);
        let grew = widened != current;
        self.provisional.insert(callee, widened);
        grew
    }

    pub(super) fn group_from(&self, order: usize) -> Vec<BindingId> {
        self.stack
            .iter()
            .copied()
            .filter(|callee| self.orders[callee] >= order)
            .collect()
    }

    pub(super) fn any_assumed(&self, group: &[BindingId]) -> bool {
        group.iter().any(|callee| self.assumed.contains(callee))
    }

    pub(super) fn close(&mut self, group: &[BindingId]) -> Vec<(BindingId, CalleeParameters)> {
        self.stack.truncate(self.stack.len() - group.len());
        group
            .iter()
            .map(|callee| (*callee, self.closed(*callee)))
            .collect()
    }

    fn closed(&mut self, callee: BindingId) -> CalleeParameters {
        self.orders.remove(&callee);
        self.assumed.remove(&callee);
        self.provisional.remove(&callee).unwrap_or_default()
    }
}
