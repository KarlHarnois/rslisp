use std::collections::HashSet;

use crate::core::BindingId;
use crate::types::{TypedDefinition, TypedExprKind, TypedLeaf, TypedProgram};

#[derive(Default)]
pub struct StoredDefinitions {
    bindings: HashSet<BindingId>,
}

impl StoredDefinitions {
    pub fn of(program: &TypedProgram) -> Self {
        let bindings = program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .filter(|definition| Self::stores_value(definition))
            .map(|definition| definition.binding)
            .collect();
        Self { bindings }
    }

    pub fn contains(&self, binding: BindingId) -> bool {
        self.bindings.contains(&binding)
    }

    fn stores_value(definition: &TypedDefinition) -> bool {
        !matches!(
            definition.value.kind,
            TypedExprKind::Fn(_) | TypedExprKind::Leaf(TypedLeaf::Rust(_))
        )
    }
}
