use rustc_hash::{FxHashMap, FxHashSet};

use super::callers::Callers;
use super::functions::{FunctionInfo, FunctionTable, ParamMode, ParamSlot};
use super::tail_calls::TailCalls;
use super::usage::{Usage, UsageCollector};
use super::worklist::Worklist;
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

    pub fn run(&self) -> FxHashMap<BindingId, ParamMode> {
        let mut modes = self.initial_modes();
        let mut callers = Callers::default();
        let mut pending = self.seed();
        while let Some(binding) = pending.pop() {
            let usage = self.usage(binding, &modes);
            callers.record(binding, &usage.callees);
            if Self::raise_modes(usage, &mut modes) {
                pending.extend([binding]);
                pending.extend(callers.of_function(binding).iter().copied());
            }
        }
        modes
    }

    fn seed(&self) -> Worklist {
        Worklist::of(self.functions.sorted_definition_bindings().into_iter())
    }

    fn initial_modes(&self) -> FxHashMap<BindingId, ParamMode> {
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

    fn initial_mode(&self, slot: &ParamSlot, rebuilt: &FxHashSet<BindingId>) -> ParamMode {
        if !slot.inferable {
            FunctionTable::fixed_mode(slot, &self.copy)
        } else if rebuilt.contains(&slot.binding) && !slot.ty.is_unsized() {
            ParamMode::Own
        } else {
            ParamMode::Ref
        }
    }

    fn usage(&self, binding: BindingId, modes: &FxHashMap<BindingId, ParamMode>) -> Usage {
        let function = self
            .functions
            .definition(binding)
            .expect("the worklist holds definitions");
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
        UsageCollector::new(self.functions, modes, &self.copy, tracked, self_modes)
            .collect(&function.lambda.body)
    }

    fn raise_modes(usage: Usage, modes: &mut FxHashMap<BindingId, ParamMode>) -> bool {
        let mut changed = false;
        for (param, needed) in usage.required {
            if needed > modes[&param] {
                modes.insert(param, needed);
                changed = true;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::ModeInference;
    use crate::core::BindingId;
    use crate::ownership::functions::FunctionTable;
    use crate::types::{CopyRules, TypedProgram, TypedSource};

    struct SeedFixture {
        program: TypedProgram,
        functions: FunctionTable,
    }

    impl SeedFixture {
        const RANGES: &str = "
            (defn leaf-range ((kilowatt-hours f64)) (* kilowatt-hours 6.5))
            (defn ariya-range ((kilowatt-hours f64)) (* kilowatt-hours 5.8))
            (defn sakura-range ((kilowatt-hours f64)) (* kilowatt-hours 6.9))
            (defn kona-range ((kilowatt-hours f64)) (* kilowatt-hours 6.2))
            (defn niro-range ((kilowatt-hours f64)) (* kilowatt-hours 5.9))
            (defn bolt-range ((kilowatt-hours f64)) (* kilowatt-hours 6.1))
            (defn zoe-range ((kilowatt-hours f64)) (* kilowatt-hours 7.1))
            (defn ioniq-range ((kilowatt-hours f64)) (* kilowatt-hours 6.4))
            (defn polestar-range ((kilowatt-hours f64)) (* kilowatt-hours 5.2))
            (defn taycan-range ((kilowatt-hours f64)) (* kilowatt-hours 4.8))
            (defn enyaq-range ((kilowatt-hours f64)) (* kilowatt-hours 5.5))
            (defn main ()
              (println (leaf-range 40.0) (ariya-range 63.0) (sakura-range 20.0))
              (println (kona-range 64.0) (niro-range 64.8) (bolt-range 65.0))
              (println (zoe-range 52.0) (ioniq-range 77.4) (polestar-range 78.0))
              (println (taycan-range 93.4) (enyaq-range 82.0)))";

        fn new() -> Self {
            let program = TypedSource::expand(Self::RANGES).typed();
            let functions = FunctionTable::collect(&program);
            Self { program, functions }
        }

        fn seeded_order(&self) -> Vec<BindingId> {
            let copy = CopyRules::new(
                &self.program.traits,
                &self.program.generics,
                &self.program.structs,
                &self.program.enums,
            );
            let mut seed = ModeInference::new(&self.functions, copy).seed();
            std::iter::from_fn(|| seed.pop()).collect()
        }
    }

    #[test]
    fn mode_inference_seeds_definitions_in_binding_order() {
        assert!(SeedFixture::new().seeded_order().is_sorted());
    }

    #[test]
    fn mode_inference_seeds_every_definition() {
        let fixture = SeedFixture::new();
        assert_eq!(
            fixture.seeded_order().len(),
            fixture.functions.definitions().count()
        );
    }
}
