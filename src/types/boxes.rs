use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::enums::{EnumTable, VariantPayload};
use super::structs::StructTable;
use super::ty::{GenericId, GenericParam, Type};
use crate::core::BindingId;

struct Declaration<'a> {
    binding: BindingId,
    params: &'a [GenericParam],
    payloads: Vec<&'a Type>,
}

struct BoxTarget {
    binding: BindingId,
    variant: usize,
    payload: usize,
    ty: Type,
}

struct Slot {
    owner: BindingId,
    targets: BTreeSet<BindingId>,
}

struct EnumSlot {
    binding: BindingId,
    variant: usize,
    payload: usize,
    owner: BindingId,
    ty: Type,
    targets: BTreeSet<BindingId>,
}

pub(super) struct BoxPlanner {
    structs: StructTable,
    enums: EnumTable,
    box_binding: BindingId,
    stored: HashMap<(BindingId, usize), bool>,
    struct_slots: Vec<Slot>,
    enum_slots: Vec<EnumSlot>,
}

impl BoxPlanner {
    pub(super) fn add_boxes(
        structs: &mut StructTable,
        enums: &mut EnumTable,
        box_binding: BindingId,
    ) -> BTreeSet<(BindingId, usize, usize)> {
        let plan = Self::new(structs.clone(), enums.clone(), box_binding).plan();
        let mut boxed = BTreeSet::new();
        for target in plan {
            let definition = enums
                .get_mut(target.binding)
                .expect("a box target names an enum");
            let payload = &mut definition.variants[target.variant].payload;
            if let VariantPayload::Positional(types) = payload {
                types[target.payload] = target.ty.clone();
            }
            if let VariantPayload::Named(fields) = payload {
                fields[target.payload].1 = target.ty;
            }
            boxed.insert((target.binding, target.variant, target.payload));
        }
        boxed
    }

    fn new(structs: StructTable, enums: EnumTable, box_binding: BindingId) -> Self {
        let declarations = Self::declarations(&structs, &enums);
        let stored = Self::stored_parameters(&declarations);
        let mut planner = Self {
            structs,
            enums,
            box_binding,
            stored,
            struct_slots: Vec::new(),
            enum_slots: Vec::new(),
        };
        planner.collect_slots();
        planner
    }

    fn plan(&self) -> Vec<BoxTarget> {
        let graph = self.graph(&BTreeSet::new());
        let candidates: Vec<usize> = self
            .enum_slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| self.is_candidate(slot, &graph))
            .map(|(index, _)| index)
            .collect();
        self.minimal_boxes(candidates)
            .into_iter()
            .map(|index| {
                let slot = &self.enum_slots[index];
                BoxTarget {
                    binding: slot.binding,
                    variant: slot.variant,
                    payload: slot.payload,
                    ty: Type::Named(self.box_binding, vec![slot.ty.clone()]),
                }
            })
            .collect()
    }

    fn minimal_boxes(&self, candidates: Vec<usize>) -> BTreeSet<usize> {
        let mut kept: BTreeSet<usize> = candidates.iter().copied().collect();
        for candidate in candidates {
            let mut trial = kept.clone();
            trial.remove(&candidate);
            if !self.has_cycle(&trial) {
                kept = trial;
            }
        }
        kept
    }

    fn is_candidate(
        &self,
        slot: &EnumSlot,
        graph: &BTreeMap<BindingId, BTreeSet<BindingId>>,
    ) -> bool {
        slot.targets
            .iter()
            .any(|target| Self::reaches(*target, slot.owner, graph))
    }

    fn has_cycle(&self, boxed: &BTreeSet<usize>) -> bool {
        let graph = self.graph(boxed);
        let mut visiting = BTreeSet::new();
        let mut visited = BTreeSet::new();
        graph
            .keys()
            .any(|binding| Self::cyclic(*binding, &graph, &mut visiting, &mut visited))
    }

    fn cyclic(
        binding: BindingId,
        graph: &BTreeMap<BindingId, BTreeSet<BindingId>>,
        visiting: &mut BTreeSet<BindingId>,
        visited: &mut BTreeSet<BindingId>,
    ) -> bool {
        if visited.contains(&binding) {
            return false;
        }
        if !visiting.insert(binding) {
            return true;
        }
        let cycle = graph
            .get(&binding)
            .into_iter()
            .flatten()
            .any(|target| Self::cyclic(*target, graph, visiting, visited));
        visiting.remove(&binding);
        visited.insert(binding);
        cycle
    }

    fn graph(&self, boxed: &BTreeSet<usize>) -> BTreeMap<BindingId, BTreeSet<BindingId>> {
        let mut graph: BTreeMap<BindingId, BTreeSet<BindingId>> = self
            .structs
            .iter()
            .map(|(binding, _)| binding)
            .chain(self.enums.iter().map(|(binding, _)| binding))
            .map(|binding| (binding, BTreeSet::new()))
            .collect();
        for slot in &self.struct_slots {
            graph
                .entry(slot.owner)
                .or_default()
                .extend(slot.targets.iter().copied());
        }
        for (index, slot) in self.enum_slots.iter().enumerate() {
            if !boxed.contains(&index) {
                graph
                    .entry(slot.owner)
                    .or_default()
                    .extend(slot.targets.iter().copied());
            }
        }
        graph
    }

    fn reaches(
        start: BindingId,
        goal: BindingId,
        graph: &BTreeMap<BindingId, BTreeSet<BindingId>>,
    ) -> bool {
        if start == goal {
            return true;
        }
        let mut seen = BTreeSet::new();
        let mut pending = vec![start];
        while let Some(binding) = pending.pop() {
            if !seen.insert(binding) {
                continue;
            }
            if let Some(targets) = graph.get(&binding) {
                if targets.contains(&goal) {
                    return true;
                }
                pending.extend(targets);
            }
        }
        false
    }

    fn declarations<'a>(
        structs: &'a StructTable,
        enums: &'a EnumTable,
    ) -> BTreeMap<BindingId, Declaration<'a>> {
        let mut declarations = BTreeMap::new();
        for (binding, definition) in structs.iter() {
            declarations.insert(
                binding,
                Declaration {
                    binding,
                    params: &definition.params,
                    payloads: definition.fields.iter().map(|(_, ty)| ty).collect(),
                },
            );
        }
        for (binding, definition) in enums.iter() {
            declarations.insert(
                binding,
                Declaration {
                    binding,
                    params: &definition.params,
                    payloads: definition.payload_types().collect(),
                },
            );
        }
        declarations
    }

    fn stored_parameters(
        declarations: &BTreeMap<BindingId, Declaration<'_>>,
    ) -> HashMap<(BindingId, usize), bool> {
        let mut stored = HashMap::new();
        loop {
            let mut changed = false;
            for declaration in declarations.values() {
                for (index, param) in declaration.params.iter().enumerate() {
                    let key = (declaration.binding, index);
                    if stored.get(&key).copied().unwrap_or(false) {
                        continue;
                    }
                    if declaration
                        .payloads
                        .iter()
                        .any(|payload| Self::mentions_at_value(payload, param.id, &stored))
                    {
                        stored.insert(key, true);
                        changed = true;
                    }
                }
            }
            if !changed {
                return stored;
            }
        }
    }

    fn mentions_at_value(
        ty: &Type,
        generic: GenericId,
        stored: &HashMap<(BindingId, usize), bool>,
    ) -> bool {
        match ty {
            Type::Generic(found) => *found == generic,
            Type::Named(binding, arguments) => {
                arguments.iter().enumerate().any(|(index, argument)| {
                    stored.get(&(*binding, index)).copied().unwrap_or(false)
                        && Self::mentions_at_value(argument, generic, stored)
                })
            }
            _ => false,
        }
    }

    fn collect_slots(&mut self) {
        for binding in self.declaration_bindings() {
            if let Some(definition) = self.structs.get(binding) {
                let slots = self.collect_struct_slots(binding, definition);
                self.struct_slots.extend(slots);
            } else if let Some(definition) = self.enums.get(binding) {
                let slots = self.collect_enum_slots(binding, definition);
                self.enum_slots.extend(slots);
            }
        }
    }

    fn collect_struct_slots(
        &self,
        binding: BindingId,
        definition: &super::StructDefinition,
    ) -> Vec<Slot> {
        definition
            .fields
            .iter()
            .map(|(_, ty)| {
                let mut targets = BTreeSet::new();
                self.collect_targets(ty, &mut targets);
                Slot {
                    owner: binding,
                    targets,
                }
            })
            .collect()
    }

    fn collect_enum_slots(
        &self,
        binding: BindingId,
        definition: &super::EnumDefinition,
    ) -> Vec<EnumSlot> {
        definition
            .variants
            .iter()
            .enumerate()
            .flat_map(|(variant, declaration)| {
                Self::variant_types(&declaration.payload)
                    .enumerate()
                    .map(move |(payload, ty)| {
                        let mut targets = BTreeSet::new();
                        self.collect_targets(ty, &mut targets);
                        EnumSlot {
                            binding,
                            variant,
                            payload,
                            owner: binding,
                            ty: ty.clone(),
                            targets,
                        }
                    })
            })
            .collect()
    }

    fn declaration_bindings(&self) -> Vec<BindingId> {
        let mut bindings: BTreeSet<BindingId> =
            self.structs.iter().map(|(binding, _)| binding).collect();
        bindings.extend(self.enums.iter().map(|(binding, _)| binding));
        bindings.into_iter().collect()
    }

    fn variant_types(payload: &VariantPayload) -> Box<dyn Iterator<Item = &Type> + '_> {
        match payload {
            VariantPayload::None => Box::new(std::iter::empty()),
            VariantPayload::Positional(types) => Box::new(types.iter()),
            VariantPayload::Named(fields) => Box::new(fields.iter().map(|(_, ty)| ty)),
        }
    }

    fn collect_targets(&self, ty: &Type, targets: &mut BTreeSet<BindingId>) {
        let Type::Named(binding, arguments) = ty else {
            return;
        };
        if self.structs.get(*binding).is_none() && self.enums.get(*binding).is_none() {
            return;
        }
        targets.insert(*binding);
        for (index, argument) in arguments.iter().enumerate() {
            if self
                .stored
                .get(&(*binding, index))
                .copied()
                .unwrap_or(false)
            {
                self.collect_targets(argument, targets);
            }
        }
    }
}
