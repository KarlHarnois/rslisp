use std::ops::Range;
use std::rc::Rc;

use rustc_hash::FxHashMap;

use super::{Scope, ScopeSet};
use crate::core::{BindingId, Namespace};
use crate::reader::Symbol;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

#[derive(serde::Serialize, serde::Deserialize)]
struct Entry {
    scopes: ScopeSet,
    binding: BindingId,
    phase: Phase,
}

impl Entry {
    fn is_visible_from_scopes(&self, scopes: &ScopeSet, phase: Phase) -> bool {
        self.phase_matches(phase) && self.scopes.is_subset_of(scopes)
    }

    fn is_visible_from_plain_scopes(&self, scopes: &ScopeSet, phase: Phase) -> bool {
        self.phase_matches(phase) && self.scopes.is_plain_subset_of(scopes)
    }

    fn phase_matches(&self, phase: Phase) -> bool {
        phase == Phase::CompileTime || self.phase == Phase::Runtime
    }

    fn anchor(&self) -> Option<Scope> {
        self.scopes.plain_largest()
    }
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct NameEntries {
    plain_sorted_by_anchor: Vec<Entry>,
    sharing: Vec<Entry>,
}

impl NameEntries {
    const SHORT_LIST_LENGTH: usize = 16;

    fn bind(
        &mut self,
        scopes: &ScopeSet,
        binding: BindingId,
        phase: Phase,
    ) -> Result<(), BindingId> {
        if !self.is_plain_search(scopes) {
            return self.bind_sharing(scopes, binding, phase);
        }
        let anchored_end = self.anchored_end(scopes.plain_largest());
        match self.entry_with_scopes(anchored_end, scopes) {
            Some(existing) => Self::existing(existing, binding),
            None => {
                let entry = Entry {
                    scopes: scopes.clone(),
                    binding,
                    phase,
                };
                self.plain_sorted_by_anchor.insert(anchored_end, entry);
                Ok(())
            }
        }
    }

    fn existing(existing: &Entry, binding: BindingId) -> Result<(), BindingId> {
        if existing.binding == binding {
            Ok(())
        } else {
            Err(existing.binding)
        }
    }

    #[inline(never)]
    fn bind_sharing(
        &mut self,
        scopes: &ScopeSet,
        binding: BindingId,
        phase: Phase,
    ) -> Result<(), BindingId> {
        if let Some(existing) = self.all().find(|entry| entry.scopes == *scopes) {
            return Self::existing(existing, binding);
        }
        let entry = Entry {
            scopes: scopes.clone(),
            binding,
            phase,
        };
        if scopes.is_plain() {
            let anchored_end = self.anchored_end(scopes.plain_largest());
            self.plain_sorted_by_anchor.insert(anchored_end, entry);
        } else {
            self.sharing.push(entry);
        }
        Ok(())
    }

    fn anchored_end(&self, anchor: Option<Scope>) -> usize {
        self.plain_sorted_by_anchor
            .partition_point(|entry| entry.anchor() <= anchor)
    }

    fn entry_with_scopes(&self, anchored_end: usize, scopes: &ScopeSet) -> Option<&Entry> {
        let anchor = scopes.plain_largest();
        self.plain_sorted_by_anchor[..anchored_end]
            .iter()
            .rev()
            .take_while(|entry| entry.anchor() == anchor)
            .find(|entry| entry.scopes == *scopes)
    }

    fn visible(&self, scopes: &ScopeSet, phase: Phase) -> Vec<&Entry> {
        let mut visible = Vec::new();
        self.visit_visible(scopes, phase, |entry| visible.push(entry));
        visible
    }

    fn visit_visible<'entries>(
        &'entries self,
        scopes: &ScopeSet,
        phase: Phase,
        visit: impl FnMut(&'entries Entry),
    ) {
        if self.is_plain_search(scopes) {
            self.visit_plain_visible(scopes, phase, visit);
        } else {
            self.visit_visible_sharing(scopes, phase, visit);
        }
    }

    fn is_plain_search(&self, scopes: &ScopeSet) -> bool {
        self.sharing.is_empty() && scopes.is_plain()
    }

    fn visit_plain_visible<'entries>(
        &'entries self,
        scopes: &ScopeSet,
        phase: Phase,
        mut visit: impl FnMut(&'entries Entry),
    ) {
        let mut visit_if_visible = |entry: &'entries Entry| {
            if entry.is_visible_from_plain_scopes(scopes, phase) {
                visit(entry);
            }
        };
        if self.is_shorter_than_an_anchor_search(scopes) {
            self.plain_sorted_by_anchor
                .iter()
                .for_each(visit_if_visible);
            return;
        }
        for anchor in Self::possible_anchors(scopes) {
            self.plain_sorted_by_anchor[self.anchored(anchor)]
                .iter()
                .for_each(&mut visit_if_visible);
        }
    }

    #[inline(never)]
    fn visit_visible_sharing<'entries>(
        &'entries self,
        scopes: &ScopeSet,
        phase: Phase,
        visit: impl FnMut(&'entries Entry),
    ) {
        self.all()
            .filter(|entry| entry.is_visible_from_scopes(scopes, phase))
            .for_each(visit);
    }

    fn all(&self) -> impl Iterator<Item = &Entry> {
        self.plain_sorted_by_anchor.iter().chain(&self.sharing)
    }

    fn is_shorter_than_an_anchor_search(&self, scopes: &ScopeSet) -> bool {
        self.plain_sorted_by_anchor.len() <= scopes.plain_len().max(Self::SHORT_LIST_LENGTH)
    }

    fn anchored(&self, anchor: Option<Scope>) -> Range<usize> {
        let start = self
            .plain_sorted_by_anchor
            .partition_point(|entry| entry.anchor() < anchor);
        let length =
            self.plain_sorted_by_anchor[start..].partition_point(|entry| entry.anchor() == anchor);
        start..start + length
    }

    fn possible_anchors(scopes: &ScopeSet) -> impl Iterator<Item = Option<Scope>> + '_ {
        std::iter::once(None).chain(scopes.plain_iter().map(Some))
    }
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct BindingTable {
    entries: FxHashMap<(Rc<str>, Namespace), NameEntries>,
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
        let mut largest = LargestCandidates::default();
        if let Some(entries) = self.entries.get(&(symbol.shared_name(), namespace)) {
            entries.visit_visible(symbol.scopes(), phase, |entry| largest.consider(entry));
        }
        largest.resolution()
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
}

#[derive(Default)]
struct LargestCandidates {
    size: usize,
    first: Option<BindingId>,
    others: Vec<BindingId>,
}

impl LargestCandidates {
    fn consider(&mut self, entry: &Entry) {
        let size = entry.scopes.len();
        match self.first {
            Some(_) if size < self.size => {}
            Some(first) if size == self.size => self.add_equal(first, entry.binding),
            _ => self.replace(size, entry.binding),
        }
    }

    fn add_equal(&mut self, first: BindingId, binding: BindingId) {
        if binding != first {
            self.others.push(binding);
        }
    }

    fn replace(&mut self, size: usize, binding: BindingId) {
        self.size = size;
        self.first = Some(binding);
        self.others.clear();
    }

    fn resolution(self) -> Resolution {
        match self.first {
            None => Resolution::Unbound,
            Some(binding) if self.others.is_empty() => Resolution::Bound(binding),
            Some(binding) => Resolution::Ambiguous(Self::distinct(binding, self.others)),
        }
    }

    fn distinct(first: BindingId, mut others: Vec<BindingId>) -> Vec<BindingId> {
        others.push(first);
        others.sort();
        others.dedup();
        others
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{BindingArena, BindingInfo, BindingKind};
    use crate::scope::{ChainPrefix, Scope, ScopeAllocator, ScopeChain};

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

        fn chain(&mut self, length: usize) -> ScopeChain {
            let chain = ScopeChain::default();
            (0..length).for_each(|_| chain.push(self.scope()));
            chain
        }

        fn prefix(chain: &ScopeChain) -> ChainPrefix {
            chain.prefix().expect("a test chain holds scopes")
        }

        fn sharing_symbol(name: &str, prefix: &ChainPrefix) -> Symbol {
            let mut scopes = ScopeSet::default();
            scopes.add_prefix(prefix);
            Symbol::with_scopes(name, scopes)
        }

        fn define_sharing(&mut self, name: &str, prefix: &ChainPrefix) -> BindingId {
            let binding = self.new_binding(name);
            self.table
                .bind(
                    &Self::sharing_symbol(name, prefix),
                    Namespace::Value,
                    binding,
                    Phase::Runtime,
                )
                .expect("no duplicate");
            binding
        }

        fn new_binding(&mut self, name: &str) -> BindingId {
            let info = BindingInfo::new(name.into(), BindingKind::Local, None);
            self.bindings.add(info)
        }

        fn resolve_sharing(&self, name: &str, prefix: &ChainPrefix) -> Resolution {
            self.table.resolve(
                &Self::sharing_symbol(name, prefix),
                Namespace::Value,
                Phase::Runtime,
            )
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
            let binding = self.new_binding(name);
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
    fn a_larger_binding_settles_two_smaller_bindings_of_equal_size() {
        let mut world = World::new();
        let (module, left, right) = (world.scope(), world.scope(), world.scope());
        world.define("x", &[module, left], Phase::Runtime);
        world.define("x", &[module, right], Phase::Runtime);
        let settled = world.define("x", &[module, left, right], Phase::Runtime);
        assert_eq!(
            world.resolve("x", &[module, left, right], Phase::Runtime),
            Resolution::Bound(settled)
        );
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
    fn rebinding_a_binding_to_its_own_scopes_is_accepted() {
        let mut world = World::new();
        let module = world.scope();
        let binding = world.define("x", &[module], Phase::Runtime);
        let rebound = world.table.bind(
            &World::symbol("x", &[module]),
            Namespace::Value,
            binding,
            Phase::Runtime,
        );
        assert_eq!(rebound, Ok(()));
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

    #[test]
    fn a_binding_under_a_shared_prefix_is_visible_from_a_longer_prefix() {
        let mut world = World::new();
        let chain = world.chain(ScopeSet::SHORTEST_SHARED_PREFIX);
        let bound = world.define_sharing("x", &World::prefix(&chain));
        chain.push(world.scope());
        assert_eq!(
            world.resolve_sharing("x", &World::prefix(&chain)),
            Resolution::Bound(bound)
        );
    }

    #[test]
    fn a_shared_binding_is_visible_from_plain_scopes_that_hold_its_prefix() {
        let mut world = World::new();
        let scopes: Vec<Scope> = (0..ScopeSet::SHORTEST_SHARED_PREFIX)
            .map(|_| world.scope())
            .collect();
        let chain = ScopeChain::default();
        scopes.iter().for_each(|scope| chain.push(*scope));
        let bound = world.define_sharing("x", &World::prefix(&chain));
        assert_eq!(
            world.resolve("x", &scopes, Phase::Runtime),
            Resolution::Bound(bound)
        );
    }

    #[test]
    fn a_compile_time_shared_binding_is_invisible_to_runtime_code() {
        let mut world = World::new();
        let prefix = World::prefix(&world.chain(ScopeSet::SHORTEST_SHARED_PREFIX));
        let binding = world.new_binding("x");
        world
            .table
            .bind(
                &World::sharing_symbol("x", &prefix),
                Namespace::Value,
                binding,
                Phase::CompileTime,
            )
            .expect("no duplicate");
        assert_eq!(world.resolve_sharing("x", &prefix), Resolution::Unbound);
    }

    #[test]
    fn a_plain_binding_beside_a_shared_one_sees_only_its_own_scopes() {
        let mut world = World::new();
        let chain = world.chain(ScopeSet::SHORTEST_SHARED_PREFIX);
        world.define_sharing("x", &World::prefix(&chain));
        let outside = world.scope();
        let plain = world.define("x", &[outside], Phase::Runtime);
        assert_eq!(
            world.resolve("x", &[outside], Phase::Runtime),
            Resolution::Bound(plain)
        );
    }

    #[test]
    fn a_shared_prefix_sees_the_plain_binding_it_shadows_only_when_larger() {
        let mut world = World::new();
        let chain = world.chain(ScopeSet::SHORTEST_SHARED_PREFIX);
        let first = World::prefix(&chain).first();
        world.define("x", &[first], Phase::Runtime);
        let shadow = world.define_sharing("x", &World::prefix(&chain));
        assert_eq!(
            world.resolve_sharing("x", &World::prefix(&chain)),
            Resolution::Bound(shadow)
        );
    }

    #[test]
    fn rebinding_shared_scopes_reports_the_existing_binding() {
        let mut world = World::new();
        let chain = world.chain(ScopeSet::SHORTEST_SHARED_PREFIX);
        let prefix = World::prefix(&chain);
        let first = world.define_sharing("x", &prefix);
        let second = world.new_binding("x");
        let duplicate = world.table.bind(
            &World::sharing_symbol("x", &prefix),
            Namespace::Value,
            second,
            Phase::Runtime,
        );
        assert_eq!(duplicate, Err(first));
    }
}
