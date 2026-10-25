use std::collections::{HashMap, HashSet};

use super::enums::EnumTable;
use super::size::TypeSizes;
use super::structs::StructTable;
use super::traits::TraitTable;
use super::ty::{Bound, GenericId, GenericParam, Type};
use crate::core::{BindingArena, BindingId, CopyTrait};

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
        enums: &'a EnumTable,
    ) -> Self {
        Self {
            traits,
            generics,
            sizes: TypeSizes::new(structs, enums),
            copy_traits: traits.with_paths(&[CopyTrait::PATH]),
        }
    }

    pub fn copy_trait_name<'b>(&self, bindings: &'b BindingArena) -> &'b str {
        let binding = self
            .copy_traits
            .first()
            .expect("the prelude defines a copy trait");
        bindings.get(*binding).name()
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::CopyRules;
    use crate::core::{BindingArena, BindingInfo, BindingKind, ModuleId, PrimitiveType};
    use crate::types::Type;
    use crate::types::structs::StructTable;
    use crate::types::traits::{Implementor, RustTrait, TraitTable};
    use crate::types::ty::Scheme;

    struct CopyTraitFixture {
        traits: TraitTable,
        structs: StructTable,
        enums: crate::types::EnumTable,
        generics: HashMap<crate::types::GenericId, crate::types::GenericParam>,
        bound: crate::core::BindingId,
    }

    impl CopyTraitFixture {
        fn new() -> Self {
            let mut bindings = BindingArena::default();
            let bound = bindings.add(BindingInfo::new(
                "copy".into(),
                BindingKind::Trait(ModuleId::new(0)),
                None,
            ));
            let mut traits = TraitTable::default();
            traits.add(
                bound,
                RustTrait {
                    path: "Copy".into(),
                    implementors: vec![Implementor {
                        scheme: Scheme::monomorphic(Type::Primitive(PrimitiveType::I64)),
                        argument: None,
                    }],
                    help: None,
                },
            );
            Self {
                traits,
                structs: StructTable::default(),
                enums: crate::types::EnumTable::default(),
                generics: HashMap::new(),
                bound,
            }
        }

        fn rules(&self) -> CopyRules<'_> {
            CopyRules::new(&self.traits, &self.generics, &self.structs, &self.enums)
        }
    }

    #[test]
    fn a_trait_whose_implementors_are_copy_allows_only_copy_values() {
        let fixture = CopyTraitFixture::new();
        assert!(fixture.rules().bound_allows_only_copy_values(fixture.bound));
    }
}
