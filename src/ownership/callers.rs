use std::collections::{HashMap, HashSet};

use crate::core::BindingId;

#[derive(Default)]
pub struct Callers {
    callers: HashMap<BindingId, Vec<BindingId>>,
    recorded: HashSet<BindingId>,
}

impl Callers {
    pub fn record(&mut self, caller: BindingId, callees: &HashSet<BindingId>) {
        if !self.recorded.insert(caller) {
            return;
        }
        for callee in callees {
            self.callers.entry(*callee).or_default().push(caller);
        }
    }

    pub fn of_function(&self, callee: BindingId) -> &[BindingId] {
        self.callers.get(&callee).map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::Callers;
    use crate::core::{BindingArena, BindingId, BindingInfo, BindingKind, ModuleId};

    struct CallerFixture {
        bindings: BindingArena,
        callers: Callers,
    }

    impl CallerFixture {
        fn new() -> Self {
            Self {
                bindings: BindingArena::default(),
                callers: Callers::default(),
            }
        }

        fn function(&mut self, name: &str) -> BindingId {
            let kind = BindingKind::Global(ModuleId::new(0));
            self.bindings.add(BindingInfo::new(name.into(), kind, None))
        }

        fn record(&mut self, caller: BindingId, callees: &[BindingId]) {
            let callees: HashSet<BindingId> = callees.iter().copied().collect();
            self.callers.record(caller, &callees);
        }
    }

    #[test]
    fn a_callee_lists_each_function_that_calls_it() {
        let mut fixture = CallerFixture::new();
        let inner = fixture.function("inner");
        let middle = fixture.function("middle");
        let outer = fixture.function("outer");
        fixture.record(middle, &[inner]);
        fixture.record(outer, &[middle, inner]);
        assert_eq!(fixture.callers.of_function(inner), [middle, outer]);
    }

    #[test]
    fn a_caller_visited_again_is_listed_once() {
        let mut fixture = CallerFixture::new();
        let inner = fixture.function("inner");
        let middle = fixture.function("middle");
        fixture.record(middle, &[inner]);
        fixture.record(middle, &[inner]);
        assert_eq!(fixture.callers.of_function(inner), [middle]);
    }
}
