use std::rc::Rc;

use rustc_hash::FxHashMap;

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
    traits: FxHashMap<BindingId, RustTrait>,
    projections: Rc<ProjectionTable>,
    normalized_implementors: FxHashMap<BindingId, Vec<Implementor>>,
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

    pub fn implies(&self, declared: &Bound, required: &Bound, implementor: GenericId) -> bool {
        self.brings(declared, &|brought| brought.meets(required, implementor))
    }

    pub fn brings(&self, declared: &Bound, wanted: &dyn Fn(&Bound) -> bool) -> bool {
        wanted(declared)
            || self.get(declared.binding).is_some_and(|rust_trait| {
                rust_trait
                    .supertraits
                    .iter()
                    .any(|supertrait| self.brings(supertrait, wanted))
            })
    }

    pub fn implemented_exactly_by(&self, types: &[Type]) -> Option<BindingId> {
        let mut matching = self
            .traits
            .iter()
            .filter(|(_, rust_trait)| Self::implementors_are(rust_trait, types))
            .map(|(binding, _)| *binding);
        let found = matching.next()?;
        matching.next().is_none().then_some(found)
    }

    fn implementors_are(rust_trait: &RustTrait, types: &[Type]) -> bool {
        rust_trait.implementors.len() == types.len()
            && types.iter().all(|ty| {
                rust_trait
                    .implementors
                    .iter()
                    .any(|implementor| implementor.scheme.body == *ty)
            })
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

    pub fn has_associated_type(&self, binding: BindingId) -> bool {
        let declares_projection = !self.projections.declared_by(binding).is_empty();
        declares_projection
            || self
                .get(binding)
                .is_some_and(|rust_trait| rust_trait.associated.is_some())
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

#[cfg(test)]
mod tests {
    use crate::core::{BindingArena, BindingId, BindingInfo, BindingKind, ModuleId, PrimitiveType};
    use crate::types::{Bound, GenericId, Implementor, RustTrait, Scheme, TraitTable, Type};

    struct TraitsFixture {
        bindings: BindingArena,
        traits: TraitTable,
    }

    impl TraitsFixture {
        fn new() -> Self {
            Self {
                bindings: BindingArena::default(),
                traits: TraitTable::default(),
            }
        }

        fn add(&mut self, name: &str, implementors: &[PrimitiveType]) -> BindingId {
            let binding = self.bindings.add(BindingInfo::new(
                name.into(),
                BindingKind::Trait(ModuleId::new(0)),
                None,
            ));
            self.traits.add(
                binding,
                RustTrait {
                    path: std::rc::Rc::from(name),
                    implementors: implementors
                        .iter()
                        .map(|primitive| Implementor {
                            scheme: Scheme::monomorphic(Type::Primitive(*primitive)),
                            argument: None,
                        })
                        .collect(),
                    help: None,
                    supertraits: Vec::new(),
                    associated: None,
                },
            );
            binding
        }
    }

    #[test]
    fn the_trait_implemented_by_exactly_the_types_is_found() {
        let mut fixture = TraitsFixture::new();
        let numbers = [PrimitiveType::I64, PrimitiveType::U8];
        fixture.add(
            "printable",
            &[PrimitiveType::I64, PrimitiveType::U8, PrimitiveType::Bool],
        );
        fixture.add("textual", &[PrimitiveType::Char, PrimitiveType::Bool]);
        let integer = fixture.add("integer", &numbers);
        let types = numbers.map(Type::Primitive);
        assert_eq!(fixture.traits.implemented_exactly_by(&types), Some(integer));
    }

    #[test]
    fn two_traits_implemented_by_exactly_the_types_find_none() {
        let mut fixture = TraitsFixture::new();
        let numbers = [PrimitiveType::I64, PrimitiveType::U8];
        fixture.add("integer", &numbers);
        fixture.add("countable", &numbers);
        let types = numbers.map(Type::Primitive);
        assert_eq!(fixture.traits.implemented_exactly_by(&types), None);
    }

    #[test]
    fn a_bound_with_a_second_type_implies_itself_and_not_another_second_type() {
        let mut fixture = TraitsFixture::new();
        let compared = fixture.add("PartialEq", &[]);
        let implementor = GenericId::new(0);
        let with_string = Bound::with_argument(compared, Type::Primitive(PrimitiveType::String));
        let with_str = Bound::with_argument(compared, Type::Primitive(PrimitiveType::Str));
        assert!(
            fixture
                .traits
                .implies(&with_string, &with_string, implementor)
        );
        assert!(!fixture.traits.implies(&with_string, &with_str, implementor));
    }
}
