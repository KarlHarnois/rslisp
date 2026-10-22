use std::collections::HashMap;

use super::functions::{FunctionTable, ParamMode};
use super::usage::UsageCollector;
use crate::core::BindingId;
use crate::types::CopyRules;

pub struct ModeInference<'a> {
    functions: &'a FunctionTable,
    copy: CopyRules<'a>,
}

impl<'a> ModeInference<'a> {
    pub fn new(functions: &'a FunctionTable, copy: CopyRules<'a>) -> Self {
        Self { functions, copy }
    }

    pub fn run(&self) -> HashMap<BindingId, ParamMode> {
        let mut modes = self.initial_modes();
        while self.raise_modes(&mut modes) {}
        modes
    }

    fn initial_modes(&self) -> HashMap<BindingId, ParamMode> {
        self.functions
            .iter()
            .flat_map(|(_, function)| &function.params)
            .map(|slot| {
                let mode = if slot.inferable {
                    ParamMode::Ref
                } else {
                    FunctionTable::fixed_mode(slot, &self.copy)
                };
                (slot.binding, mode)
            })
            .collect()
    }

    fn raise_modes(&self, modes: &mut HashMap<BindingId, ParamMode>) -> bool {
        let mut changed = false;
        for (_, function) in self.functions.iter() {
            let tracked = function
                .params
                .iter()
                .filter(|slot| slot.inferable && !slot.ty.is_unsized())
                .map(|slot| slot.binding)
                .collect();
            let self_modes = function
                .params
                .iter()
                .map(|slot| modes[&slot.binding])
                .collect();
            let required = UsageCollector::new(self.functions, modes, tracked, self_modes)
                .collect(&function.lambda.body);
            for (binding, needed) in required {
                let current = modes[&binding];
                if needed > current {
                    modes.insert(binding, needed);
                    changed = true;
                }
            }
        }
        changed
    }
}
