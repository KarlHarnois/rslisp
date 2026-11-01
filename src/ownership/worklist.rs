use std::collections::VecDeque;

use rustc_hash::FxHashSet;

use crate::core::BindingId;

pub struct Worklist {
    queue: VecDeque<BindingId>,
    queued: FxHashSet<BindingId>,
}

impl Worklist {
    pub fn of(bindings: impl Iterator<Item = BindingId>) -> Self {
        let mut worklist = Self {
            queue: VecDeque::new(),
            queued: FxHashSet::default(),
        };
        worklist.extend(bindings);
        worklist
    }

    pub fn extend(&mut self, bindings: impl IntoIterator<Item = BindingId>) {
        for binding in bindings {
            if self.queued.insert(binding) {
                self.queue.push_back(binding);
            }
        }
    }

    pub fn pop(&mut self) -> Option<BindingId> {
        let binding = self.queue.pop_front()?;
        self.queued.remove(&binding);
        Some(binding)
    }
}
