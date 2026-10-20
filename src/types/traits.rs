use std::collections::HashMap;
use std::rc::Rc;

use super::ty::Type;
use crate::core::BindingId;

pub struct RustTrait {
    pub path: Rc<str>,
    pub implementors: Vec<Type>,
}

#[derive(Default)]
pub struct TraitTable {
    traits: HashMap<BindingId, RustTrait>,
}

impl TraitTable {
    pub fn add(&mut self, binding: BindingId, rust_trait: RustTrait) {
        self.traits.insert(binding, rust_trait);
    }

    pub fn is_implemented(&self, binding: BindingId, implementor: &Type) -> bool {
        self.traits
            .get(&binding)
            .is_some_and(|rust_trait| rust_trait.implementors.contains(implementor))
    }

    pub fn add_implementor_to_paths(&mut self, paths: &[&str], implementor: Type) {
        for rust_trait in self.traits.values_mut() {
            if paths.contains(&rust_trait.path.as_ref()) {
                rust_trait.implementors.push(implementor.clone());
            }
        }
    }

    pub fn get(&self, binding: BindingId) -> Option<&RustTrait> {
        self.traits.get(&binding)
    }
}
