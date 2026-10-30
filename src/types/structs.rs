use std::collections::HashMap;
use std::rc::Rc;

use super::ty::{GenericId, GenericParam, Type};
use crate::core::BindingId;
use crate::source::Span;

#[derive(Clone)]
pub struct StructDefinition {
    pub lifetimes: Vec<BindingId>,
    pub params: Vec<GenericParam>,
    pub fields: Vec<(Rc<str>, Type)>,
    pub field_spans: Vec<Span>,
    pub function_spans: Vec<Span>,
    pub derives: Vec<&'static str>,
    pub derived_traits: Vec<BindingId>,
    pub blocked: Vec<(BindingId, Rc<str>, Type)>,
    pub written_derive: Option<Span>,
}

impl StructDefinition {
    pub fn field(&self, name: &str) -> Option<&Type> {
        self.fields
            .iter()
            .find(|(field, _)| field.as_ref() == name)
            .map(|(_, field_type)| field_type)
    }

    pub fn instantiated_field(&self, name: &str, arguments: &[Type]) -> Option<Type> {
        let replacements = self.replacements(arguments);
        self.field(name).map(|field_type| {
            field_type
                .without_written_lifetimes()
                .substitute(&replacements)
        })
    }

    pub fn instantiated_fields(&self, arguments: &[Type]) -> Vec<(Rc<str>, Type)> {
        let replacements = self.replacements(arguments);
        self.fields
            .iter()
            .map(|(name, field_type)| {
                (
                    name.clone(),
                    field_type
                        .without_written_lifetimes()
                        .substitute(&replacements),
                )
            })
            .collect()
    }

    fn replacements(&self, arguments: &[Type]) -> HashMap<GenericId, Type> {
        self.params
            .iter()
            .map(|param| param.id)
            .zip(arguments.iter().cloned())
            .collect()
    }
}

#[derive(Clone, Default)]
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

    pub fn get_mut(&mut self, binding: BindingId) -> Option<&mut StructDefinition> {
        self.structs.get_mut(&binding)
    }

    pub fn iter(&self) -> impl Iterator<Item = (BindingId, &StructDefinition)> {
        self.structs
            .iter()
            .map(|(binding, definition)| (*binding, definition))
    }

    pub fn with_field(&self, name: &str) -> Vec<BindingId> {
        self.structs
            .iter()
            .filter(|(_, definition)| definition.field(name).is_some())
            .map(|(binding, _)| *binding)
            .collect()
    }
}
