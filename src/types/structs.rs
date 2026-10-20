use std::collections::HashMap;
use std::rc::Rc;

use super::ty::Type;
use crate::core::BindingId;

pub struct StructDefinition {
    pub fields: Vec<(Rc<str>, Type)>,
}

impl StructDefinition {
    pub fn field(&self, name: &str) -> Option<&Type> {
        self.fields
            .iter()
            .find(|(field, _)| field.as_ref() == name)
            .map(|(_, field_type)| field_type)
    }
}

#[derive(Default)]
pub struct StructTable {
    structs: HashMap<BindingId, StructDefinition>,
}

impl StructTable {
    pub fn add(&mut self, binding: BindingId, definition: StructDefinition) {
        self.structs.insert(binding, definition);
    }

    pub fn get(&self, binding: BindingId) -> Option<&StructDefinition> {
        self.structs.get(&binding)
    }

    pub fn with_field(&self, name: &str) -> Vec<BindingId> {
        self.structs
            .iter()
            .filter(|(_, definition)| definition.field(name).is_some())
            .map(|(binding, _)| *binding)
            .collect()
    }
}
