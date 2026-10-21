use std::collections::HashMap;

use super::traits::TraitTable;
use super::ty::{GenericId, GenericParam, Type};
use crate::core::{BindingId, CopyTrait};

pub struct CopyRules<'a> {
    traits: &'a TraitTable,
    generics: &'a HashMap<GenericId, GenericParam>,
    copy_traits: Vec<BindingId>,
}

impl<'a> CopyRules<'a> {
    pub fn new(traits: &'a TraitTable, generics: &'a HashMap<GenericId, GenericParam>) -> Self {
        Self {
            traits,
            generics,
            copy_traits: traits.with_paths(&[CopyTrait::PATH]),
        }
    }

    pub fn is_copy(&self, value: &Type) -> bool {
        self.copy_traits.iter().any(|copy| {
            self.traits
                .satisfies(*copy, value, &|generic, _| self.generic_is_copy(generic))
        })
    }

    fn generic_is_copy(&self, generic: GenericId) -> bool {
        self.generics.get(&generic).is_some_and(|param| {
            param.bounds.iter().any(|bound| {
                self.copy_traits.contains(bound) || self.only_copy_implementors(*bound)
            })
        })
    }

    fn only_copy_implementors(&self, bound: BindingId) -> bool {
        self.traits.get(bound).is_some_and(|rust_trait| {
            !rust_trait.implementors.is_empty()
                && rust_trait.implementors.iter().all(|implementor| {
                    implementor.params.is_empty() && self.is_copy(&implementor.body)
                })
        })
    }
}
