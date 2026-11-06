use rustc_hash::{FxHashMap, FxHashSet};

use crate::core::BindingId;
use crate::types::{Scheme, Type, TypedProgram};

pub struct UncoercedMethodParameters {
    positions: FxHashMap<BindingId, FxHashSet<usize>>,
}

impl UncoercedMethodParameters {
    pub fn of(program: &TypedProgram) -> Self {
        let receiverless_methods: FxHashSet<BindingId> = program
            .trait_definitions
            .iter()
            .flat_map(|definition| &definition.methods)
            .filter(|method| method.receiver.is_none())
            .map(|method| method.binding)
            .collect();
        let positions = program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .filter(|definition| receiverless_methods.contains(&definition.binding))
            .map(|definition| (definition.binding, Self::positions(&definition.scheme)))
            .collect();
        Self { positions }
    }

    pub fn contains(&self, method: BindingId, position: usize) -> bool {
        self.positions
            .get(&method)
            .is_some_and(|positions| positions.contains(&position))
    }

    fn positions(scheme: &Scheme) -> FxHashSet<usize> {
        let Type::Function(_, parameters, _) = &scheme.body else {
            unreachable!("a trait method has a function type")
        };
        parameters
            .iter()
            .enumerate()
            .filter(|(_, parameter)| parameter.is_generic_behind_shared_references())
            .map(|(position, _)| position)
            .collect()
    }
}
