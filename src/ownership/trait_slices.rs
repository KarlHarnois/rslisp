use std::collections::{HashMap, HashSet};

use super::functions::FunctionTable;
use super::refusals::Refusals;
use crate::core::BindingId;
use crate::types::{Type, TypedProgram};

pub struct TraitSlot {
    pub method: BindingId,
    pub position: usize,
    pub declared: Type,
    pub parameters: Vec<(BindingId, Type)>,
}

impl TraitSlot {
    pub fn is_at(&self, method: BindingId, position: usize) -> bool {
        self.method == method && self.position == position
    }

    pub fn is_refused(&self, refused: &HashSet<BindingId>) -> bool {
        self.bindings()
            .any(|parameter| refused.contains(&parameter))
    }

    pub fn bind_mates(&self, refusals: &mut Refusals) {
        let leaders = self.bindings().cycle().skip(1);
        for (follower, leader) in self.bindings().zip(leaders) {
            refusals.follow(follower, leader);
        }
    }

    fn bindings(&self) -> impl Iterator<Item = BindingId> + Clone + '_ {
        self.parameters.iter().map(|(binding, _)| *binding)
    }
}

pub struct TraitSlotCollector<'a> {
    program: &'a TypedProgram,
    functions: &'a FunctionTable,
    declared: &'a HashMap<BindingId, Vec<Type>>,
}

impl<'a> TraitSlotCollector<'a> {
    pub fn new(
        program: &'a TypedProgram,
        functions: &'a FunctionTable,
        declared: &'a HashMap<BindingId, Vec<Type>>,
    ) -> Self {
        Self {
            program,
            functions,
            declared,
        }
    }

    pub fn slots(&self) -> Vec<TraitSlot> {
        self.program
            .trait_definitions
            .iter()
            .flat_map(|definition| {
                definition
                    .methods
                    .iter()
                    .flat_map(|method| self.method_slots(definition.binding, method.binding))
            })
            .collect()
    }

    fn method_slots(&self, trait_binding: BindingId, method: BindingId) -> Vec<TraitSlot> {
        let name = self.program.bindings.get(method).name();
        let bodies: Vec<_> = self
            .program
            .trait_method_bodies(trait_binding, name)
            .into_iter()
            .filter_map(|body| self.functions.definition(body))
            .collect();
        self.declared
            .get(&method)
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(position, declared)| TraitSlot {
                method,
                position,
                declared: declared.clone(),
                parameters: bodies
                    .iter()
                    .filter_map(|body| body.params.get(position))
                    .map(|slot| (slot.binding, slot.ty.clone()))
                    .collect(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::TraitSlot;
    use crate::core::{BindingArena, BindingId, BindingInfo, BindingKind, ModuleId, PrimitiveType};
    use crate::ownership::refusals::Refusals;
    use crate::types::Type;

    struct SlotFixture {
        bindings: BindingArena,
    }

    impl SlotFixture {
        fn new() -> Self {
            Self {
                bindings: BindingArena::default(),
            }
        }

        fn binding(&mut self, name: &str) -> BindingId {
            let kind = BindingKind::Global(ModuleId::new(0));
            self.bindings.add(BindingInfo::new(name.into(), kind, None))
        }

        fn slot(&mut self, parameters: Vec<BindingId>) -> TraitSlot {
            let declared = Type::Primitive(PrimitiveType::I64);
            TraitSlot {
                method: self.binding("total"),
                position: 1,
                parameters: parameters
                    .into_iter()
                    .map(|parameter| (parameter, declared.clone()))
                    .collect(),
                declared,
            }
        }
    }

    #[test]
    fn a_method_without_bodies_keeps_its_slot() {
        let mut fixture = SlotFixture::new();
        let slot = fixture.slot(Vec::new());
        assert!(!slot.is_refused(&HashSet::new()));
    }

    #[test]
    fn one_refused_body_refuses_the_slot() {
        let mut fixture = SlotFixture::new();
        let reading = fixture.binding("readings");
        let pushing = fixture.binding("pushed");
        let slot = fixture.slot(vec![reading, pushing]);
        assert!(slot.is_refused(&HashSet::from([pushing])));
    }

    #[test]
    fn a_refused_body_refuses_every_mate() {
        let mut fixture = SlotFixture::new();
        let reading = fixture.binding("readings");
        let summing = fixture.binding("summed");
        let pushing = fixture.binding("pushed");
        let slot = fixture.slot(vec![reading, summing, pushing]);
        let mut refusals = Refusals::default();
        slot.bind_mates(&mut refusals);
        refusals.refuse(pushing);
        assert_eq!(
            refusals.spread(),
            HashSet::from([reading, summing, pushing])
        );
    }
}
