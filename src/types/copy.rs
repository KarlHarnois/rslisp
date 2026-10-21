use std::collections::HashMap;

use super::traits::TraitTable;
use super::ty::{GenericId, GenericParam, Type};
use crate::core::PrimitiveType;

pub struct CopyRules<'a> {
    traits: &'a TraitTable,
    generics: &'a HashMap<GenericId, GenericParam>,
}

impl<'a> CopyRules<'a> {
    pub fn new(traits: &'a TraitTable, generics: &'a HashMap<GenericId, GenericParam>) -> Self {
        Self { traits, generics }
    }

    pub fn is_copy(&self, value: &Type) -> bool {
        match value {
            Type::Primitive(primitive) => {
                !matches!(primitive, PrimitiveType::String | PrimitiveType::Str)
            }
            Type::Ref(_) => true,
            Type::Generic(generic) => self.generic_is_copy(*generic),
            Type::Mut(_) | Type::Named(..) | Type::Function(..) | Type::Var(_) => false,
        }
    }

    fn generic_is_copy(&self, generic: GenericId) -> bool {
        self.generics.get(&generic).is_some_and(|param| {
            param.bounds.iter().any(|bound| {
                self.traits.get(*bound).is_some_and(|rust_trait| {
                    !rust_trait.implementors.is_empty()
                        && rust_trait.implementors.iter().all(|implementor| {
                            implementor.params.is_empty() && self.is_copy(&implementor.body)
                        })
                })
            })
        })
    }
}
