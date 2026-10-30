use std::cell::{Ref, RefCell};
use std::rc::Rc;

use super::Scope;

#[derive(Clone, Default)]
pub struct ScopeChain {
    scopes: Rc<RefCell<Vec<Scope>>>,
}

impl ScopeChain {
    pub fn push(&self, scope: Scope) {
        let mut scopes = self.scopes.borrow_mut();
        assert!(
            scopes.last() < Some(&scope),
            "a scope chain takes only fresh scopes, in order"
        );
        scopes.push(scope);
    }

    pub fn prefix(&self) -> Option<ChainPrefix> {
        let scopes = self.scopes.borrow();
        Some(ChainPrefix {
            chain: self.clone(),
            length: scopes.len(),
            first: *scopes.first()?,
            last: *scopes.last()?,
        })
    }
}

#[derive(Clone)]
pub struct ChainPrefix {
    chain: ScopeChain,
    length: usize,
    first: Scope,
    last: Scope,
}

impl ChainPrefix {
    pub(super) fn len(&self) -> usize {
        self.length
    }

    pub(super) fn first(&self) -> Scope {
        self.first
    }

    pub(super) fn last(&self) -> Scope {
        self.last
    }

    #[inline]
    pub(super) fn contains(&self, scope: Scope) -> bool {
        (self.first..=self.last).contains(&scope) && self.scopes().binary_search(&scope).is_ok()
    }

    pub(super) fn all_from(&self, start: usize, predicate: impl Fn(Scope) -> bool) -> bool {
        self.scopes()[start..].iter().all(|scope| predicate(*scope))
    }

    pub(super) fn for_each(&self, act: impl FnMut(Scope)) {
        self.scopes().iter().copied().for_each(act);
    }

    pub(super) fn to_vec(&self) -> Vec<Scope> {
        self.scopes().to_vec()
    }

    fn shares_chain_with(&self, other: &ChainPrefix) -> bool {
        Rc::ptr_eq(&self.chain.scopes, &other.chain.scopes)
    }

    fn scopes(&self) -> Ref<'_, [Scope]> {
        Ref::map(self.chain.scopes.borrow(), |scopes| &scopes[..self.length])
    }
}

impl PartialEq for ChainPrefix {
    fn eq(&self, other: &Self) -> bool {
        self.shares_chain_with(other) && self.length == other.length
    }
}

#[derive(Clone, Default, PartialEq)]
pub(super) struct SharedScopes {
    ascending_prefixes: Option<Rc<Vec<ChainPrefix>>>,
}

impl SharedScopes {
    pub(super) fn is_empty(&self) -> bool {
        self.ascending_prefixes.is_none()
    }

    pub(super) fn push(&mut self, prefix: ChainPrefix) {
        Rc::make_mut(self.ascending_prefixes.get_or_insert_default()).push(prefix);
    }

    pub(super) fn len(&self) -> usize {
        self.prefixes().iter().map(ChainPrefix::len).sum()
    }

    pub(super) fn largest(&self) -> Option<Scope> {
        self.prefixes().last().map(ChainPrefix::last)
    }

    #[inline(never)]
    pub(super) fn contains(&self, scope: Scope) -> bool {
        self.prefixes().iter().any(|prefix| prefix.contains(scope))
    }

    pub(super) fn covered_length(&self, prefix: &ChainPrefix) -> usize {
        self.prefixes()
            .iter()
            .filter(|shared| shared.shares_chain_with(prefix))
            .map(|shared| shared.length.min(prefix.length))
            .max()
            .unwrap_or(0)
    }

    pub(super) fn prefixes(&self) -> &[ChainPrefix] {
        self.ascending_prefixes
            .as_deref()
            .map_or(&[], Vec::as_slice)
    }

    pub(super) fn ascending(&self) -> Vec<Scope> {
        self.prefixes()
            .iter()
            .flat_map(ChainPrefix::to_vec)
            .collect()
    }

    pub(super) fn splits(&self, keep: &impl Fn(Scope) -> bool) -> bool {
        self.prefixes().iter().any(|prefix| {
            let scopes = prefix.scopes();
            scopes.iter().any(|scope| keep(*scope)) && !scopes.iter().all(|scope| keep(*scope))
        })
    }

    pub(super) fn retain_kept(&mut self, keep: &impl Fn(Scope) -> bool) {
        if let Some(prefixes) = self.ascending_prefixes.as_mut() {
            Rc::make_mut(prefixes).retain(|prefix| keep(prefix.first()));
            if prefixes.is_empty() {
                self.ascending_prefixes = None;
            }
        }
    }
}
