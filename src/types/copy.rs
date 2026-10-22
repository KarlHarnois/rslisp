use std::collections::{HashMap, HashSet};

use super::size::TypeSizes;
use super::structs::StructTable;
use super::traits::TraitTable;
use super::ty::{Bound, GenericId, GenericParam, Type};
use crate::core::{BindingId, CopyTrait};

#[derive(Clone, Default)]
pub struct InferredCopies {
    parameters: HashSet<BindingId>,
}

impl InferredCopies {
    pub fn insert(&mut self, binding: BindingId) {
        self.parameters.insert(binding);
    }

    pub fn extend(&mut self, other: InferredCopies) {
        self.parameters.extend(other.parameters);
    }

    pub fn contains(&self, binding: BindingId) -> bool {
        self.parameters.contains(&binding)
    }

    pub fn is_empty(&self) -> bool {
        self.parameters.is_empty()
    }
}

pub struct CopyRules<'a> {
    traits: &'a TraitTable,
    generics: &'a HashMap<GenericId, GenericParam>,
    sizes: TypeSizes<'a>,
    copy_traits: Vec<BindingId>,
}

impl<'a> CopyRules<'a> {
    pub fn new(
        traits: &'a TraitTable,
        generics: &'a HashMap<GenericId, GenericParam>,
        structs: &'a StructTable,
    ) -> Self {
        Self {
            traits,
            generics,
            sizes: TypeSizes::new(structs),
            copy_traits: traits.with_paths(&[CopyTrait::PATH]),
        }
    }

    pub fn is_copy(&self, value: &Type) -> bool {
        self.copy_traits.iter().any(|copy| {
            self.traits
                .satisfies(&Bound::plain(*copy), value, &|generic, _| {
                    self.generic_is_copy(generic)
                })
        })
    }

    pub fn is_small_copy(&self, value: &Type) -> bool {
        self.is_copy(value) && self.sizes.is_small(value)
    }

    fn generic_is_copy(&self, generic: GenericId) -> bool {
        self.generics.get(&generic).is_some_and(|param| {
            param.bounds.iter().any(|bound| {
                self.copy_traits.contains(&bound.binding)
                    || self.bound_allows_only_copy(bound.binding)
            })
        })
    }

    pub fn bound_allows_only_copy_values(&self, bound: BindingId) -> bool {
        self.bound_allows_only_copy(bound)
            && self.traits.get(bound).is_some_and(|rust_trait| {
                rust_trait
                    .implementors
                    .iter()
                    .all(|implementor| !implementor.scheme.body.is_reference())
            })
    }

    fn bound_allows_only_copy(&self, bound: BindingId) -> bool {
        self.traits.get(bound).is_some_and(|rust_trait| {
            !rust_trait.implementors.is_empty()
                && rust_trait.implementors.iter().all(|implementor| {
                    implementor.scheme.params.is_empty() && self.is_copy(&implementor.scheme.body)
                })
        })
    }
}
