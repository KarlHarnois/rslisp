use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::core::BindingId;
use crate::types::ClosureKind;

#[derive(Clone, Default)]
pub struct ParameterUses {
    calls: HashMap<BindingId, usize>,
    uses: HashMap<BindingId, usize>,
    pass_limits: HashMap<BindingId, ClosureKind>,
    required: HashMap<BindingId, ClosureKind>,
}

impl ParameterUses {
    pub fn joined(branches: Vec<Self>) -> Self {
        let mut joined = Self::default();
        for branch in branches {
            joined.join(branch);
        }
        joined
    }

    pub fn record_call(&mut self, parameter: BindingId) {
        *self.calls.entry(parameter).or_insert(0) += 1;
        *self.uses.entry(parameter).or_insert(0) += 1;
    }

    pub fn record_pass(&mut self, parameter: BindingId, callee_kind: ClosureKind) {
        *self.uses.entry(parameter).or_insert(0) += 1;
        let limit = self.pass_limits.entry(parameter).or_insert(callee_kind);
        *limit = (*limit).min(callee_kind);
    }

    pub fn require(&mut self, parameter: BindingId, kind: ClosureKind) {
        let current = self.required.entry(parameter).or_insert(ClosureKind::Fn);
        *current = (*current).max(kind);
    }

    pub fn extend(&mut self, other: Self) {
        Self::combine_with(&mut self.calls, other.calls, usize::saturating_add);
        Self::combine_with(&mut self.uses, other.uses, usize::saturating_add);
        Self::combine_with(&mut self.pass_limits, other.pass_limits, ClosureKind::min);
        Self::combine_with(&mut self.required, other.required, ClosureKind::max);
    }

    pub fn counts(&self) -> Self {
        Self {
            calls: self.calls.clone(),
            uses: self.uses.clone(),
            ..Self::default()
        }
    }

    pub fn repeat_uses_since(&mut self, before: &Self) {
        self.extend(self.uses_since(before));
    }

    pub fn uses_since(&self, before: &Self) -> Self {
        Self {
            calls: Self::added_since(&self.calls, &before.calls),
            uses: Self::added_since(&self.uses, &before.uses),
            ..Self::default()
        }
    }

    pub fn kind(&self, parameter: BindingId) -> ClosureKind {
        let required = self
            .required
            .get(&parameter)
            .copied()
            .unwrap_or(ClosureKind::Fn);
        self.weakest_allowed_kind(parameter)
            .unwrap_or(ClosureKind::Fn)
            .max(required)
    }

    pub fn allows_fn_once(&self, parameter: BindingId) -> bool {
        self.weakest_allowed_kind(parameter)
            .is_none_or(|kind| kind == ClosureKind::FnOnce)
    }

    fn join(&mut self, branch: Self) {
        Self::combine_with(&mut self.calls, branch.calls, usize::max);
        Self::combine_with(&mut self.uses, branch.uses, usize::max);
        Self::combine_with(&mut self.pass_limits, branch.pass_limits, ClosureKind::min);
        Self::combine_with(&mut self.required, branch.required, ClosureKind::max);
    }

    fn combine_with<T: Copy>(
        joined: &mut HashMap<BindingId, T>,
        other: HashMap<BindingId, T>,
        combine: fn(T, T) -> T,
    ) {
        for (parameter, value) in other {
            match joined.entry(parameter) {
                Entry::Occupied(mut entry) => {
                    let current = entry.get_mut();
                    *current = combine(*current, value);
                }
                Entry::Vacant(entry) => {
                    entry.insert(value);
                }
            }
        }
    }

    fn added_since(
        counts: &HashMap<BindingId, usize>,
        before: &HashMap<BindingId, usize>,
    ) -> HashMap<BindingId, usize> {
        counts
            .iter()
            .map(|(parameter, count)| {
                let earlier = before.get(parameter).copied().unwrap_or(0);
                (*parameter, count.saturating_sub(earlier))
            })
            .collect()
    }

    fn weakest_allowed_kind(&self, parameter: BindingId) -> Option<ClosureKind> {
        let pass_limit = self.pass_limits.get(&parameter).copied();
        pass_limit
            .into_iter()
            .chain(self.call_kind(parameter))
            .min()
    }

    fn call_kind(&self, parameter: BindingId) -> Option<ClosureKind> {
        let calls = self.calls.get(&parameter).copied().unwrap_or(0);
        let uses = self.uses.get(&parameter).copied().unwrap_or(0);
        match (calls, uses) {
            (0, _) => None,
            (_, 1) => Some(ClosureKind::FnOnce),
            _ => Some(ClosureKind::FnMut),
        }
    }
}
