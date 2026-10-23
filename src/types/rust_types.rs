use std::collections::BTreeMap;
use std::rc::Rc;

use crate::core::{BindingId, ParsedTemplate};

pub struct RustType {
    pub template: Rc<ParsedTemplate>,
}

impl RustType {
    pub const SLICE_TEMPLATE: &'static str = "[{0}]";
    pub const VECTOR_TEMPLATE: &'static str = "Vec<{0}>";
    const OPAQUE_PREFIX: &'static str = "impl ";

    pub fn is_opaque(&self) -> bool {
        self.template.text().starts_with(Self::OPAQUE_PREFIX)
    }

    pub fn has_template(&self, template: &str) -> bool {
        self.template
            .text()
            .chars()
            .filter(|character| !character.is_whitespace())
            .eq(template.chars())
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
