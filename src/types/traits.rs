use std::collections::HashMap;
use std::rc::Rc;

use super::projection::ProjectionTable;
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
    pub associated: Option<Rc<str>>,
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
    projections: Rc<ProjectionTable>,
    normalized_implementors: HashMap<BindingId, Vec<Implementor>>,
}

impl TraitTable {
    pub fn projections_mut(&mut self) -> &mut ProjectionTable {
        Rc::get_mut(&mut self.projections).expect("projections are shared only after collection")
    }

    pub fn share_projections(&mut self) -> Rc<ProjectionTable> {
        for (binding, rust_trait) in &self.traits {
            let normalized: Vec<Implementor> = rust_trait
                .implementors
                .iter()
                .filter_map(|implementor| Self::normalized(&self.projections, implementor))
                .collect();
            if !normalized.is_empty() {
                self.normalized_implementors.insert(*binding, normalized);
            }
        }
        Rc::clone(&self.projections)
    }

    fn normalized(projections: &ProjectionTable, implementor: &Implementor) -> Option<Implementor> {
        let body = projections.normalize(&implementor.scheme.body)?;
        Some(Implementor {
            scheme: Scheme {
                body,
                ..implementor.scheme.clone()
            },
            argument: implementor.argument.clone(),
        })
    }

    pub fn projections(&self) -> &ProjectionTable {
        &self.projections
    }

    pub fn add(&mut self, binding: BindingId, rust_trait: RustTrait) {
        self.traits.insert(binding, rust_trait);
    }

    pub fn select(&self, bound: &Bound, implementor: &Type) -> Selection {
        let normalized = self.projections.normalize_deep(implementor);
        self.traits.get(&bound.binding).map_or_else(
            || Selection::Missing,
            |rust_trait| {
                rust_trait
                    .implementors
                    .iter()
                    .chain(self.normalized_implementors(bound.binding))
                    .map(|known| {
                        ImplementorMatch::select(
                            known,
                            bound,
                            &normalized,
                            rust_trait.associated.is_some(),
                        )
                    })
                    .fold(Selection::Missing, Selection::best)
            },
        )
    }

    fn normalized_implementors(&self, binding: BindingId) -> &[Implementor] {
        self.normalized_implementors
            .get(&binding)
            .map(Vec::as_slice)
            .unwrap_or_default()
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
            Selection::Implemented { requirements, .. } => requirements.iter().all(|requirement| {
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
