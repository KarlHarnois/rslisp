use std::collections::{HashMap, HashSet};

use super::functions::{FunctionInfo, FunctionTable, ParamMode, ParamSlot};
use super::tail_calls::TailCalls;
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
            .flat_map(|(binding, function)| self.function_modes(*binding, function))
            .collect()
    }

    fn function_modes(
        &self,
        binding: BindingId,
        function: &FunctionInfo,
    ) -> Vec<(BindingId, ParamMode)> {
        let rebuilt =
            TailCalls::of(Some(binding), &function.lambda).rebuilt_parameters(&function.lambda);
        function
            .params
            .iter()
            .map(|slot| (slot.binding, self.initial_mode(slot, &rebuilt)))
            .collect()
    }

    fn initial_mode(&self, slot: &ParamSlot, rebuilt: &HashSet<BindingId>) -> ParamMode {
        if !slot.inferable {
            FunctionTable::fixed_mode(slot, &self.copy)
        } else if rebuilt.contains(&slot.binding) && !slot.ty.is_unsized() {
            ParamMode::Own
        } else {
            ParamMode::Ref
        }
    }

    fn raise_modes(&self, modes: &mut HashMap<BindingId, ParamMode>) -> bool {
        let mut changed = false;
        for function in self.functions.definitions() {
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
            let required =
                UsageCollector::new(self.functions, modes, &self.copy, tracked, self_modes)
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
