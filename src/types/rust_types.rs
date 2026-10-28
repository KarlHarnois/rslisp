use std::collections::BTreeMap;
use std::rc::Rc;

use super::ty::Type;
use crate::core::{BindingId, ParsedTemplate};

pub struct RustType {
    pub template: Rc<ParsedTemplate>,
    pub clauses: Vec<(Type, Rc<ParsedTemplate>)>,
    pub params: Vec<Rc<str>>,
    pub unsized_parameters: Vec<usize>,
}

impl RustType {
    pub const SLICE_TEMPLATE: &'static str = "[{0}]";
    pub const VECTOR_TEMPLATE: &'static str = "Vec<{0}>";
    pub const BOX_TEMPLATE: &'static str = "Box<{0}>";
    pub const ITERATOR_TEMPLATE: &'static str = "impl Iterator<Item = {0}>";
    const OPAQUE_PREFIX: &'static str = "impl ";

    pub fn template_for(&self, argument: Option<&Type>) -> &Rc<ParsedTemplate> {
        argument
            .and_then(|argument| {
                self.clauses
                    .iter()
                    .find(|(target, _)| target == argument)
                    .map(|(_, template)| template)
            })
            .unwrap_or(&self.template)
    }

    pub fn is_opaque(&self) -> bool {
        self.template.text().starts_with(Self::OPAQUE_PREFIX)
    }

    pub fn has_template(&self, template: &str) -> bool {
        self.template
            .text()
            .chars()
            .filter(|character| !character.is_whitespace())
            .eq(template
                .chars()
                .filter(|character| !character.is_whitespace()))
    }

    pub fn leading_name(&self) -> &str {
        let text = self.template.text();
        let named = text.strip_prefix(Self::OPAQUE_PREFIX).unwrap_or(text);
        let end = named
            .find(|character: char| !character.is_alphanumeric() && character != '_')
            .unwrap_or(named.len());
        &named[..end]
    }
}

#[derive(Default)]
pub struct RustTypeTable {
    types: BTreeMap<BindingId, RustType>,
}

impl RustTypeTable {
    pub fn add(&mut self, binding: BindingId, rust_type: RustType) {
        self.types.insert(binding, rust_type);
    }

    pub fn get(&self, binding: BindingId) -> Option<&RustType> {
        self.types.get(&binding)
    }

    pub fn is_slice(&self, binding: BindingId) -> bool {
        self.has_template(binding, RustType::SLICE_TEMPLATE)
    }

    pub fn is_vector(&self, binding: BindingId) -> bool {
        self.has_template(binding, RustType::VECTOR_TEMPLATE)
    }

    pub fn is_box(&self, binding: BindingId) -> bool {
        self.has_template(binding, RustType::BOX_TEMPLATE)
    }

    pub fn parameter_may_be_unsized(&self, binding: BindingId, position: usize) -> bool {
        self.get(binding)
            .is_some_and(|rust_type| rust_type.unsized_parameters.contains(&position))
    }

    pub fn is_sorted_map(&self, binding: BindingId) -> bool {
        self.has_template(binding, "std::collections::BTreeMap<{0}, {1}>")
    }

    pub fn is_hash_map(&self, binding: BindingId) -> bool {
        self.has_template(binding, "std::collections::HashMap<{0}, {1}>")
    }

    pub fn is_iterator(&self, binding: BindingId) -> bool {
        self.has_template(binding, RustType::ITERATOR_TEMPLATE)
    }

    pub fn option_binding(&self) -> Option<BindingId> {
        self.binding_with_template("Option<{0}>")
    }

    pub fn iterator_binding(&self) -> Option<BindingId> {
        self.binding_with_template(RustType::ITERATOR_TEMPLATE)
    }

    pub fn pair_binding(&self) -> Option<BindingId> {
        self.binding_with_template("({0}, {1})")
    }

    pub fn box_binding(&self) -> Option<BindingId> {
        self.binding_with_template(RustType::BOX_TEMPLATE)
    }

    pub fn binding_with_template(&self, template: &str) -> Option<BindingId> {
        self.types
            .iter()
            .find(|(_, rust_type)| rust_type.has_template(template))
            .map(|(binding, _)| *binding)
    }

    fn has_template(&self, binding: BindingId, template: &str) -> bool {
        self.get(binding)
            .is_some_and(|rust_type| rust_type.has_template(template))
    }
}
