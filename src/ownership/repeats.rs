use super::definitions::LocalDefinitions;
use super::moved_values::MovedValues;
use crate::core::BindingId;
use crate::types::TypedLambda;

pub struct RepeatingBody {
    entry: MovedValues,
    carried: MovedValues,
}

impl RepeatingBody {
    pub fn enter(moved: &MovedValues) -> Self {
        Self {
            entry: moved.clone(),
            carried: MovedValues::default(),
        }
    }

    pub fn carry(&mut self, moved: &MovedValues) {
        self.carried.merge(moved.moved_since(&self.entry));
    }

    pub fn next_pass(mut self, lambda: &TypedLambda) -> Option<NextPass> {
        if self.carried.is_empty() {
            return None;
        }
        let own = LocalDefinitions::of(lambda);
        self.carried.retain(|binding| !own.contains(&binding));
        (!self.carried.is_empty()).then_some(NextPass {
            entry: self.entry,
            carried: self.carried,
        })
    }
}

pub struct NextPass {
    entry: MovedValues,
    carried: MovedValues,
}

impl NextPass {
    pub fn carried_bindings(&self) -> impl Iterator<Item = BindingId> + '_ {
        self.carried.bindings()
    }

    pub fn moved_at_start(self) -> MovedValues {
        let mut moved = self.entry;
        moved.merge(self.carried);
        moved
    }
}
