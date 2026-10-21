use std::collections::HashMap;
use std::rc::Rc;

use crate::core::{BindingId, ParsedTemplate};

pub struct RustType {
    pub template: Rc<ParsedTemplate>,
}

impl RustType {
    const OPAQUE_PREFIX: &'static str = "impl ";

    pub fn is_opaque(&self) -> bool {
        self.template.text().starts_with(Self::OPAQUE_PREFIX)
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
    types: HashMap<BindingId, RustType>,
}

impl RustTypeTable {
    pub fn add(&mut self, binding: BindingId, rust_type: RustType) {
        self.types.insert(binding, rust_type);
    }

    pub fn get(&self, binding: BindingId) -> Option<&RustType> {
        self.types.get(&binding)
    }
}
