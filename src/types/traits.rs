use std::collections::HashMap;
use std::rc::Rc;

use super::selection::{ImplementorMatch, Selection};
use super::ty::{Scheme, Type};
use crate::core::BindingId;

pub struct RustTrait {
    pub path: Rc<str>,
    pub implementors: Vec<Scheme>,
}

impl RustTrait {
    pub fn is_open(&self) -> bool {
        self.implementors
            .iter()
            .any(|implementor| !implementor.params.is_empty())
    }
}

#[derive(Default)]
pub struct TraitTable {
    traits: HashMap<BindingId, RustTrait>,
}

impl TraitTable {
    pub fn add(&mut self, binding: BindingId, rust_trait: RustTrait) {
        self.traits.insert(binding, rust_trait);
    }

    pub fn select(&self, binding: BindingId, implementor: &Type) -> Selection {
        self.traits
            .get(&binding)
            .map_or(&[][..], |rust_trait| rust_trait.implementors.as_slice())
            .iter()
            .map(|known| ImplementorMatch::select(known, implementor))
            .fold(Selection::Missing, Selection::best)
    }

    pub fn with_paths(&self, paths: &[&str]) -> Vec<BindingId> {
        let mut found: Vec<BindingId> = self
            .traits
            .iter()
            .filter(|(_, rust_trait)| paths.contains(&rust_trait.path.as_ref()))
            .map(|(binding, _)| *binding)
            .collect();
        found.sort();
        found
    }

    pub fn add_implementor(&mut self, binding: BindingId, implementor: Scheme) {
        if let Some(rust_trait) = self.traits.get_mut(&binding) {
            rust_trait.implementors.push(implementor);
        }
    }

    pub fn get(&self, binding: BindingId) -> Option<&RustTrait> {
        self.traits.get(&binding)
    }
}
