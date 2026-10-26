use std::collections::HashMap;
use std::rc::Rc;

use super::selection::{ImplementorMatch, Selection};
use super::ty::{Bound, GenericId, Scheme, Type};
use crate::core::BindingId;

pub struct Implementor {
    pub scheme: Scheme,
    pub argument: Option<Type>,
}

pub struct RustTrait {
    pub path: Rc<str>,
    pub implementors: Vec<Implementor>,
    pub help: Option<Rc<str>>,
    pub supertraits: Vec<Bound>,
}

impl RustTrait {
    pub fn is_open(&self) -> bool {
        self.implementors
            .iter()
            .any(|implementor| !implementor.scheme.params.is_empty())
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

    pub fn select(&self, bound: &Bound, implementor: &Type) -> Selection {
        self.traits
            .get(&bound.binding)
            .map_or(&[][..], |rust_trait| rust_trait.implementors.as_slice())
            .iter()
            .map(|known| ImplementorMatch::select(known, bound, implementor))
            .fold(Selection::Missing, Selection::best)
    }

    pub fn satisfies(
        &self,
        bound: &Bound,
        candidate: &Type,
        generic_satisfies: &dyn Fn(GenericId, &Bound) -> bool,
    ) -> bool {
        if let Type::Generic(generic) = candidate {
            return generic_satisfies(*generic, bound);
        }
        match self.select(bound, candidate) {
            Selection::Implemented(requirements) => requirements.iter().all(|requirement| {
                self.satisfies(
                    &requirement.bound,
                    &requirement.implementor,
                    generic_satisfies,
                )
            }),
            Selection::Undecided | Selection::Missing => false,
        }
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

    pub fn add_implementor(&mut self, binding: BindingId, implementor: Implementor) {
        if let Some(rust_trait) = self.traits.get_mut(&binding) {
            rust_trait.implementors.push(implementor);
        }
    }

    pub fn remove_struct_implementor(&mut self, binding: BindingId, struct_binding: BindingId) {
        if let Some(rust_trait) = self.traits.get_mut(&binding) {
            rust_trait.implementors.retain(|implementor| {
                !matches!(implementor.scheme.body, Type::Named(named, _) if named == struct_binding)
            });
        }
    }

    pub fn get(&self, binding: BindingId) -> Option<&RustTrait> {
        self.traits.get(&binding)
    }

    pub fn set_implementor_scheme(&mut self, binding: BindingId, body: &Type, scheme: Scheme) {
        if let Some(rust_trait) = self.traits.get_mut(&binding)
            && let Some(implementor) = rust_trait
                .implementors
                .iter_mut()
                .find(|implementor| implementor.scheme.body == *body)
        {
            implementor.scheme = scheme;
        }
    }
}
