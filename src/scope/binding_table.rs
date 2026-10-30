use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use super::{Scope, ScopeSet};
use crate::core::{BindingId, Namespace};
use crate::reader::Symbol;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Runtime,
    CompileTime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    Bound(BindingId),
    Unbound,
    Ambiguous(Vec<BindingId>),
}

struct Entry {
    scopes: ScopeSet,
    binding: BindingId,
    phase: Phase,
}

impl Entry {
    fn is_visible_from_scopes(&self, scopes: &ScopeSet, phase: Phase) -> bool {
        let phase_matches = phase == Phase::CompileTime || self.phase == Phase::Runtime;
        phase_matches && self.scopes.is_subset_of(scopes)
    }

    fn anchor(&self) -> Option<Scope> {
        self.scopes.largest()
    }
}

#[derive(Default)]
struct NameEntries {
    sorted_by_anchor: Vec<Entry>,
}

impl NameEntries {
    const SHORT_LIST_LENGTH: usize = 16;

    fn bind(
        &mut self,
        scopes: &ScopeSet,
        binding: BindingId,
        phase: Phase,
    ) -> Result<(), BindingId> {
        let anchored_end = self.anchored_end(scopes.largest());
        match self.entry_with_scopes(anchored_end, scopes) {
            Some(existing) if existing.binding == binding => Ok(()),
            Some(existing) => Err(existing.binding),
            None => {
                let entry = Entry {
                    scopes: scopes.clone(),
                    binding,
                    phase,
                };
                self.sorted_by_anchor.insert(anchored_end, entry);
                Ok(())
            }
        }
    }

    fn anchored_end(&self, anchor: Option<Scope>) -> usize {
        self.sorted_by_anchor
            .partition_point(|entry| entry.anchor() <= anchor)
    }

    fn entry_with_scopes(&self, anchored_end: usize, scopes: &ScopeSet) -> Option<&Entry> {
        let anchor = scopes.largest();
        self.sorted_by_anchor[..anchored_end]
            .iter()
            .rev()
            .take_while(|entry| entry.anchor() == anchor)
            .find(|entry| entry.scopes == *scopes)
    }

    fn visible(&self, scopes: &ScopeSet, phase: Phase) -> Vec<&Entry> {
        let is_visible = |entry: &&Entry| entry.is_visible_from_scopes(scopes, phase);
        if self.is_shorter_than_an_anchor_search(scopes) {
            return self.sorted_by_anchor.iter().filter(is_visible).collect();
        }
        Self::possible_anchors(scopes)
            .flat_map(|anchor| &self.sorted_by_anchor[self.anchored(anchor)])
            .filter(is_visible)
            .collect()
    }

    fn is_shorter_than_an_anchor_search(&self, scopes: &ScopeSet) -> bool {
        self.sorted_by_anchor.len() <= scopes.len().max(Self::SHORT_LIST_LENGTH)
    }

    fn anchored(&self, anchor: Option<Scope>) -> Range<usize> {
        let start = self
            .sorted_by_anchor
            .partition_point(|entry| entry.anchor() < anchor);
        let length =
            self.sorted_by_anchor[start..].partition_point(|entry| entry.anchor() == anchor);
        start..start + length
    }

    fn possible_anchors(scopes: &ScopeSet) -> impl Iterator<Item = Option<Scope>> + '_ {
        std::iter::once(None).chain(scopes.iter().map(Some))
    }
}

#[derive(Default)]
pub struct BindingTable {
    entries: HashMap<(Rc<str>, Namespace), NameEntries>,
}

impl BindingTable {
    pub fn bind(
        &mut self,
        symbol: &Symbol,
        namespace: Namespace,
        binding: BindingId,
        phase: Phase,
    ) -> Result<(), BindingId> {
        self.entries
            .entry((symbol.shared_name(), namespace))
            .or_default()
            .bind(symbol.scopes(), binding, phase)
    }

    pub fn resolve(&self, symbol: &Symbol, namespace: Namespace, phase: Phase) -> Resolution {
        let candidates = self.candidates(symbol, namespace, phase);
        let Some(largest) = candidates.iter().map(|entry| entry.scopes.len()).max() else {
            return Resolution::Unbound;
        };
        let mut best: Vec<BindingId> = candidates
            .iter()
            .filter(|entry| entry.scopes.len() == largest)
            .map(|entry| entry.binding)
            .collect();
        best.sort();
        best.dedup();
        match best.as_slice() {
            [binding] => Resolution::Bound(*binding),
            _ => Resolution::Ambiguous(best),
        }
    }

    pub fn visible_name_bindings(
        &self,
        symbol: &Symbol,
        namespace: Namespace,
        phase: Phase,
    ) -> Vec<(Rc<str>, BindingId)> {
        self.entries
            .iter()
            .filter(|((_, entry_namespace), _)| *entry_namespace == namespace)
            .flat_map(|((name, _), entries)| {
                entries
                    .visible(symbol.scopes(), phase)
                    .into_iter()
                    .map(|entry| (Rc::clone(name), entry.binding))
            })
            .collect()
    }

    pub fn visible_bindings(
        &self,
        scopes: &ScopeSet,
        namespace: Namespace,
        phase: Phase,
    ) -> Vec<BindingId> {
        self.entries
            .iter()
            .filter(|((_, entry_namespace), _)| *entry_namespace == namespace)
            .flat_map(|(_, entries)| entries.visible(scopes, phase))
            .map(|entry| entry.binding)
            .collect()
    }

    fn candidates(&self, symbol: &Symbol, namespace: Namespace, phase: Phase) -> Vec<&Entry> {
        let key = (symbol.shared_name(), namespace);
        self.entries
            .get(&key)
            .map(|entries| entries.visible(symbol.scopes(), phase))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{BindingArena, BindingInfo, BindingKind};
    use crate::scope::{Scope, ScopeAllocator};

    struct World {
        table: BindingTable,
        bindings: BindingArena,
        scopes: ScopeAllocator,
    }

    impl World {
        fn new() -> Self {
            Self {
                table: BindingTable::default(),
                bindings: BindingArena::default(),
                scopes: ScopeAllocator::default(),
            }
        }

        fn scope(&mut self) -> Scope {
            self.scopes.fresh()
        }

        fn symbol(name: &str, scopes: &[Scope]) -> Symbol {
            Symbol::with_scopes(name, Self::scope_set(scopes))
        }

        fn scope_set(scopes: &[Scope]) -> ScopeSet {
            let mut set = ScopeSet::default();
            scopes.iter().for_each(|scope| set.add(*scope));
            set
        }

        fn define(&mut self, name: &str, scopes: &[Scope], phase: Phase) -> BindingId {
            let info = BindingInfo::new(name.into(), BindingKind::Local, None);
            let binding = self.bindings.add(info);
            let symbol = Self::symbol(name, scopes);
            self.table
                .bind(&symbol, Namespace::Value, binding, phase)
                .expect("no duplicate");
            binding
        }

        fn define_under_subsets(
            &mut self,
            name: &str,
            scopes: &[Scope],
            count: usize,
        ) -> Vec<(Vec<Scope>, BindingId)> {
            (0..count)
                .map(|index| {
                    let subset = Self::subset(scopes, index);
                    let binding = self.define(name, &subset, Phase::Runtime);
                    (subset, binding)
                })
                .collect()
        }

        fn subset(scopes: &[Scope], index: usize) -> Vec<Scope> {
            scopes
                .iter()
                .enumerate()
                .filter(|(position, _)| index & (1 << position) != 0)
                .map(|(_, scope)| *scope)
                .collect()
        }

        fn visible(&self, scopes: &[Scope]) -> Vec<BindingId> {
            let mut bindings = self.table.visible_bindings(
                &Self::scope_set(scopes),
                Namespace::Value,
                Phase::Runtime,
            );
            bindings.sort();
            bindings
        }

        fn resolve(&self, name: &str, scopes: &[Scope], phase: Phase) -> Resolution {
            self.table
                .resolve(&Self::symbol(name, scopes), Namespace::Value, phase)
        }
    }

    #[test]
    fn the_binding_with_the_largest_subset_wins() {
        let mut world = World::new();
        let (module, inner) = (world.scope(), world.scope());
        world.define("x", &[module], Phase::Runtime);
        let shadow = world.define("x", &[module, inner], Phase::Runtime);
        assert_eq!(
            world.resolve("x", &[module, inner], Phase::Runtime),
            Resolution::Bound(shadow)
        );
    }

    #[test]
    fn a_binding_outside_the_reference_scopes_is_invisible() {
        let mut world = World::new();
        let (module, macro_introduced) = (world.scope(), world.scope());
        world.define("tmp", &[module, macro_introduced], Phase::Runtime);
        assert_eq!(
            world.resolve("tmp", &[module], Phase::Runtime),
            Resolution::Unbound
        );
    }

    #[test]
    fn two_bindings_of_equal_size_are_ambiguous() {
        let mut world = World::new();
        let (module, left, right) = (world.scope(), world.scope(), world.scope());
        world.define("x", &[module, left], Phase::Runtime);
        world.define("x", &[module, right], Phase::Runtime);
        let resolution = world.resolve("x", &[module, left, right], Phase::Runtime);
        assert!(matches!(resolution, Resolution::Ambiguous(_)));
    }

    #[test]
    fn compile_time_bindings_are_invisible_to_runtime_code() {
        let mut world = World::new();
        let module = world.scope();
        world.define("name", &[module], Phase::CompileTime);
        assert_eq!(
            world.resolve("name", &[module], Phase::Runtime),
            Resolution::Unbound
        );
    }

    #[test]
    fn rebinding_the_same_scopes_reports_the_existing_binding() {
        let mut world = World::new();
        let module = world.scope();
        let first = world.define("x", &[module], Phase::Runtime);
        let info = BindingInfo::new("x".into(), BindingKind::Local, None);
        let second = world.bindings.add(info);
        let duplicate = world.table.bind(
            &World::symbol("x", &[module]),
            Namespace::Value,
            second,
            Phase::Runtime,
        );
        assert_eq!(duplicate, Err(first));
    }

    #[test]
    fn a_long_binding_list_finds_what_a_full_scan_finds() {
        let mut world = World::new();
        let scopes: Vec<Scope> = (0..5).map(|_| world.scope()).collect();
        let entries = world.define_under_subsets("x", &scopes, NameEntries::SHORT_LIST_LENGTH + 4);
        let reference = [scopes[0], scopes[1], scopes[4]];
        let scanned: Vec<BindingId> = entries
            .iter()
            .filter(|(subset, _)| subset.iter().all(|scope| reference.contains(scope)))
            .map(|(_, binding)| *binding)
            .collect();
        let (_, exact) = entries
            .iter()
            .find(|(subset, _)| *subset == reference)
            .expect("the reference scopes are a defined subset");
        assert_eq!(world.visible(&reference), scanned);
        assert_eq!(
            world.resolve("x", &reference, Phase::Runtime),
            Resolution::Bound(*exact)
        );
    }
}
