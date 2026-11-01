use rustc_hash::{FxHashMap, FxHashSet};

use crate::core::BindingId;
use crate::types::{TypedDefinition, TypedExprKind, TypedLeaf, TypedProgram};

#[derive(Default)]
pub struct StoredDefinitions {
    bindings: FxHashSet<BindingId>,
}

impl StoredDefinitions {
    pub fn of(program: &TypedProgram) -> Self {
        let definitions: FxHashMap<BindingId, &TypedDefinition> = program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .map(|definition| (definition.binding, definition))
            .collect();
        let bindings = definitions
            .values()
            .filter(|definition| Self::stores_value(definition, &definitions))
            .map(|definition| definition.binding)
            .collect();
        Self { bindings }
    }

    pub fn contains(&self, binding: BindingId) -> bool {
        self.bindings.contains(&binding)
    }

    fn stores_value(
        definition: &TypedDefinition,
        definitions: &FxHashMap<BindingId, &TypedDefinition>,
    ) -> bool {
        match &definition.value.kind {
            TypedExprKind::Fn(_) | TypedExprKind::Leaf(TypedLeaf::Rust(_)) => false,
            TypedExprKind::Leaf(TypedLeaf::Global(named)) => definitions
                .get(named)
                .is_none_or(|named| Self::stores_value(named, definitions)),
            _ => true,
        }
    }
}
