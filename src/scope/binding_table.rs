use std::collections::HashMap;
use std::rc::Rc;

use super::ScopeSet;
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
    fn is_visible_from(&self, symbol: &Symbol, phase: Phase) -> bool {
        self.is_visible_from_scopes(symbol.scopes(), phase)
    }

    fn is_visible_from_scopes(&self, scopes: &ScopeSet, phase: Phase) -> bool {
        let phase_matches = phase == Phase::CompileTime || self.phase == Phase::Runtime;
        phase_matches && self.scopes.is_subset_of(scopes)
    }
}

#[derive(Default)]
pub struct BindingTable {
    entries: HashMap<(Rc<str>, Namespace), Vec<Entry>>,
}

impl BindingTable {
    pub fn bind(
        &mut self,
        symbol: &Symbol,
        namespace: Namespace,
        binding: BindingId,
        phase: Phase,
    ) -> Result<(), BindingId> {
        let entries = self
            .entries
            .entry((symbol.shared_name(), namespace))
            .or_default();
        match entries
            .iter()
            .find(|entry| entry.scopes == *symbol.scopes())
        {
            Some(existing) if existing.binding == binding => Ok(()),
            Some(existing) => Err(existing.binding),
            None => {
                entries.push(Entry {
                    scopes: symbol.scopes().clone(),
                    binding,
                    phase,
                });
                Ok(())
            }
        }
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

    pub fn visible_names(
        &self,
        symbol: &Symbol,
        namespace: Namespace,
        phase: Phase,
    ) -> Vec<Rc<str>> {
        self.entries
            .iter()
            .filter(|((_, entry_namespace), _)| *entry_namespace == namespace)
            .filter(|(_, entries)| {
                entries
                    .iter()
                    .any(|entry| entry.is_visible_from(symbol, phase))
            })
            .map(|((name, _), _)| Rc::clone(name))
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
            .flat_map(|(_, entries)| entries)
            .filter(|entry| entry.is_visible_from_scopes(scopes, phase))
            .map(|entry| entry.binding)
            .collect()
    }

    fn candidates(&self, symbol: &Symbol, namespace: Namespace, phase: Phase) -> Vec<&Entry> {
        let key = (symbol.shared_name(), namespace);
        self.entries
            .get(&key)
            .map(|entries| {
                entries
                    .iter()
                    .filter(|entry| entry.is_visible_from(symbol, phase))
                    .collect()
            })
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
            let mut set = ScopeSet::default();
            scopes.iter().for_each(|scope| set.add(*scope));
            Symbol::with_scopes(name, set)
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
}
