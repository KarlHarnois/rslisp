use std::fmt;
use std::hash::{Hash, Hasher};
use std::iter::Copied;
use std::{slice, vec};

use super::scope_chain::{ChainPrefix, SharedScopes};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Scope(u32);

#[derive(Default)]
pub struct ScopeAllocator {
    next: u32,
}

impl ScopeAllocator {
    pub fn fresh(&mut self) -> Scope {
        let scope = Scope(self.next);
        self.next += 1;
        scope
    }
}

#[derive(Clone, Default)]
pub struct ScopeSet {
    sorted: SortedScopes,
}

impl ScopeSet {
    pub(super) const SHORTEST_SHARED_PREFIX: usize = 64;

    pub fn add(&mut self, scope: Scope) {
        if let Err(position) = self.position(scope)
            && !self.shares(scope)
        {
            self.sorted.insert(position, scope);
        }
    }

    pub fn add_prefix(&mut self, prefix: &ChainPrefix) {
        if self.is_worth_sharing(prefix) {
            self.sorted.share(prefix.clone());
        } else {
            prefix.for_each(|scope| self.add(scope));
        }
    }

    pub fn remove(&mut self, scope: Scope) {
        match self.position(scope) {
            Ok(position) => self.sorted.remove(position),
            Err(_) if self.shares(scope) => self.remove_shared(scope),
            Err(_) => {}
        }
    }

    pub fn retain(&mut self, keep: impl Fn(Scope) -> bool) {
        if self.shared().is_some_and(|shared| shared.splits(&keep)) {
            self.unshare();
        }
        self.sorted.retain(keep);
    }

    pub fn flip(&mut self, scope: Scope) {
        match self.position(scope) {
            Ok(position) => self.sorted.remove(position),
            Err(_) if self.shares(scope) => self.remove_shared(scope),
            Err(position) => self.sorted.insert(position, scope),
        }
    }

    pub fn contains(&self, scope: Scope) -> bool {
        self.position(scope).is_ok() || self.shares(scope)
    }

    #[inline]
    pub fn is_subset_of(&self, other: &ScopeSet) -> bool {
        if self.is_plain() && other.is_plain() {
            return self.is_plain_subset_of(other);
        }
        self.shared_is_subset_of(other)
    }

    pub(super) fn largest(&self) -> Option<Scope> {
        let own_largest = self.own().last().copied();
        match self.shared() {
            Some(shared) => own_largest.max(shared.largest()),
            None => own_largest,
        }
    }

    pub(super) fn iter(&self) -> SortedIter<'_> {
        match self.shared() {
            Some(_) => SortedIter::Merged(self.merged().into_iter()),
            None => SortedIter::Own(self.own().iter().copied()),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.own().len() + self.shared().map_or(0, SharedScopes::len)
    }

    pub(super) fn is_plain(&self) -> bool {
        self.shared().is_none()
    }

    #[inline]
    pub(super) fn is_plain_subset_of(&self, other: &ScopeSet) -> bool {
        debug_assert!(self.is_plain() && other.is_plain());
        let mut larger = other.own().iter();
        self.own()
            .iter()
            .all(|scope| larger.any(|candidate| candidate == scope))
    }

    pub(super) fn plain_largest(&self) -> Option<Scope> {
        debug_assert!(self.is_plain());
        self.own().last().copied()
    }

    pub(super) fn plain_iter(&self) -> impl Iterator<Item = Scope> {
        debug_assert!(self.is_plain());
        self.own().iter().copied()
    }

    pub(super) fn plain_len(&self) -> usize {
        debug_assert!(self.is_plain());
        self.own().len()
    }

    #[inline(never)]
    fn shared_is_subset_of(&self, other: &ScopeSet) -> bool {
        self.own().iter().all(|scope| other.contains(*scope))
            && self.shared().is_none_or(|shared| {
                shared
                    .prefixes()
                    .iter()
                    .all(|prefix| other.contains_prefix(prefix))
            })
    }

    fn is_worth_sharing(&self, prefix: &ChainPrefix) -> bool {
        prefix.len() >= Self::SHORTEST_SHARED_PREFIX && Some(prefix.first()) > self.largest()
    }

    fn contains_prefix(&self, prefix: &ChainPrefix) -> bool {
        let covered = self
            .shared()
            .map_or(0, |shared| shared.covered_length(prefix));
        prefix.all_from(covered, |scope| self.contains(scope))
    }

    #[inline(never)]
    fn remove_shared(&mut self, scope: Scope) {
        self.unshare();
        self.remove(scope);
    }

    fn merged(&self) -> Vec<Scope> {
        let mut scopes = self.own().to_vec();
        if let Some(shared) = self.shared() {
            scopes.extend(shared.ascending());
            scopes.sort_unstable();
        }
        scopes
    }

    #[inline]
    fn shares(&self, scope: Scope) -> bool {
        self.shared().is_some_and(|shared| shared.contains(scope))
    }

    fn unshare(&mut self) {
        self.sorted = SortedScopes::spilled(self.merged());
    }

    #[inline]
    fn shared(&self) -> Option<&SharedScopes> {
        self.sorted.shared()
    }

    fn own(&self) -> &[Scope] {
        self.sorted.as_slice()
    }

    fn position(&self, scope: Scope) -> Result<usize, usize> {
        self.own().binary_search(&scope)
    }
}

impl PartialEq for ScopeSet {
    fn eq(&self, other: &Self) -> bool {
        if self.shared() == other.shared() {
            return self.own() == other.own();
        }
        self.len() == other.len() && self.merged() == other.merged()
    }
}

impl Eq for ScopeSet {}

impl Hash for ScopeSet {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self.shared() {
            Some(_) => self.merged().hash(state),
            None => self.own().hash(state),
        }
    }
}

impl fmt::Debug for ScopeSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_set().entries(self.iter()).finish()
    }
}

pub(super) enum SortedIter<'set> {
    Own(Copied<slice::Iter<'set, Scope>>),
    Merged(vec::IntoIter<Scope>),
}

impl Iterator for SortedIter<'_> {
    type Item = Scope;

    fn next(&mut self) -> Option<Scope> {
        match self {
            Self::Own(scopes) => scopes.next(),
            Self::Merged(scopes) => scopes.next(),
        }
    }
}

#[derive(Clone)]
enum SortedScopes {
    Inline {
        length: usize,
        scopes: [Scope; SortedScopes::INLINE_CAPACITY],
    },
    Spilled {
        scopes: Vec<Scope>,
        shared: SharedScopes,
    },
}

impl Default for SortedScopes {
    fn default() -> Self {
        Self::Inline {
            length: 0,
            scopes: [Scope(0); Self::INLINE_CAPACITY],
        }
    }
}

impl SortedScopes {
    const INLINE_CAPACITY: usize = 8;

    fn spilled(scopes: Vec<Scope>) -> Self {
        Self::Spilled {
            scopes,
            shared: SharedScopes::default(),
        }
    }

    fn as_slice(&self) -> &[Scope] {
        match self {
            Self::Inline { length, scopes } => &scopes[..*length],
            Self::Spilled { scopes, .. } => scopes,
        }
    }

    fn shared(&self) -> Option<&SharedScopes> {
        match self {
            Self::Spilled { shared, .. } if !shared.is_empty() => Some(shared),
            Self::Inline { .. } | Self::Spilled { .. } => None,
        }
    }

    fn share(&mut self, prefix: ChainPrefix) {
        if let Self::Inline { .. } = self {
            *self = Self::spilled(self.as_slice().to_vec());
        }
        if let Self::Spilled { shared, .. } = self {
            shared.push(prefix);
        }
    }

    fn insert(&mut self, position: usize, scope: Scope) {
        match self {
            Self::Inline { length, scopes } if *length < Self::INLINE_CAPACITY => {
                scopes.copy_within(position..*length, position + 1);
                scopes[position] = scope;
                *length += 1;
            }
            Self::Inline { .. } => *self = Self::spilled(self.spilled_with(position, scope)),
            Self::Spilled { scopes, .. } => scopes.insert(position, scope),
        }
    }

    fn spilled_with(&self, position: usize, scope: Scope) -> Vec<Scope> {
        let mut scopes = self.as_slice().to_vec();
        scopes.insert(position, scope);
        scopes
    }

    fn remove(&mut self, position: usize) {
        match self {
            Self::Inline { length, scopes } => {
                scopes.copy_within(position + 1..*length, position);
                *length -= 1;
            }
            Self::Spilled { scopes, .. } => {
                scopes.remove(position);
            }
        }
    }

    fn retain(&mut self, keep: impl Fn(Scope) -> bool) {
        match self {
            Self::Inline { length, scopes } => {
                *length = Self::retain_inline(&mut scopes[..*length], keep)
            }
            Self::Spilled { scopes, shared } => {
                scopes.retain(|scope| keep(*scope));
                shared.retain_kept(&keep);
            }
        }
    }

    fn retain_inline(scopes: &mut [Scope], keep: impl Fn(Scope) -> bool) -> usize {
        let mut retained = 0;
        for index in 0..scopes.len() {
            if keep(scopes[index]) {
                scopes[retained] = scopes[index];
                retained += 1;
            }
        }
        retained
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::ScopeChain;

    struct Scopes {
        allocator: ScopeAllocator,
    }

    impl Scopes {
        const SHARED_LENGTH: usize = ScopeSet::SHORTEST_SHARED_PREFIX;

        fn new() -> Self {
            Self {
                allocator: ScopeAllocator::default(),
            }
        }

        fn set_of(&mut self, count: usize) -> (ScopeSet, Vec<Scope>) {
            let scopes: Vec<Scope> = (0..count).map(|_| self.allocator.fresh()).collect();
            let mut set = ScopeSet::default();
            scopes.iter().for_each(|scope| set.add(*scope));
            (set, scopes)
        }

        fn chain_of(&mut self, count: usize) -> (ScopeChain, Vec<Scope>) {
            let chain = ScopeChain::default();
            let scopes: Vec<Scope> = (0..count).map(|_| self.allocator.fresh()).collect();
            scopes.iter().for_each(|scope| chain.push(*scope));
            (chain, scopes)
        }

        fn shared_set_of(&mut self, count: usize) -> (ScopeSet, Vec<Scope>) {
            let (chain, scopes) = self.chain_of(count);
            let mut set = ScopeSet::default();
            set.add_prefix(&Scopes::prefix_of(&chain));
            (set, scopes)
        }

        fn prefix_of(chain: &ScopeChain) -> ChainPrefix {
            chain.prefix().expect("a test chain holds scopes")
        }

        fn added_one_by_one(scopes: &[Scope]) -> ScopeSet {
            let mut set = ScopeSet::default();
            scopes.iter().for_each(|scope| set.add(*scope));
            set
        }

        fn hash_of(set: &ScopeSet) -> u64 {
            let mut hasher = std::hash::DefaultHasher::new();
            set.hash(&mut hasher);
            hasher.finish()
        }
    }

    #[test]
    fn flipping_twice_restores_the_set() {
        let (mut set, scopes) = Scopes::new().set_of(2);
        let original = set.clone();
        set.flip(scopes[0]);
        set.flip(scopes[0]);
        assert_eq!(set, original);
    }

    #[test]
    fn a_smaller_set_is_a_subset_of_a_larger_one() {
        let (larger, scopes) = Scopes::new().set_of(3);
        let mut smaller = larger.clone();
        smaller.remove(scopes[1]);
        assert!(smaller.is_subset_of(&larger));
        assert!(!larger.is_subset_of(&smaller));
    }

    #[test]
    fn a_set_beyond_its_inline_capacity_keeps_every_scope_in_order() {
        let (set, scopes) = Scopes::new().set_of(SortedScopes::INLINE_CAPACITY + 2);
        assert_eq!(set.iter().collect::<Vec<_>>(), scopes);
    }

    #[test]
    fn scopes_added_out_of_order_come_back_sorted() {
        for count in [
            SortedScopes::INLINE_CAPACITY,
            SortedScopes::INLINE_CAPACITY + 2,
        ] {
            let (sorted, scopes) = Scopes::new().set_of(count);
            let mut reversed = ScopeSet::default();
            scopes.iter().rev().for_each(|scope| reversed.add(*scope));
            assert_eq!(reversed, sorted);
        }
    }

    #[test]
    fn removing_and_retaining_work_beyond_the_inline_capacity() {
        let (mut set, scopes) = Scopes::new().set_of(SortedScopes::INLINE_CAPACITY + 2);
        set.remove(scopes[0]);
        set.flip(scopes[1]);
        set.retain(|scope| scope != scopes[2]);
        assert_eq!(set.iter().collect::<Vec<_>>(), scopes[3..]);
    }

    #[test]
    fn retaining_an_inline_set_keeps_the_chosen_scopes() {
        let (mut set, scopes) = Scopes::new().set_of(3);
        set.retain(|scope| scope != scopes[1]);
        assert_eq!(set.iter().collect::<Vec<_>>(), [scopes[0], scopes[2]]);
    }

    #[test]
    fn a_spilled_set_equals_and_hashes_like_an_inline_set_with_the_same_scopes() {
        let (mut spilled, scopes) = Scopes::new().set_of(SortedScopes::INLINE_CAPACITY + 1);
        spilled.remove(scopes[SortedScopes::INLINE_CAPACITY]);
        let mut inline = ScopeSet::default();
        scopes[..SortedScopes::INLINE_CAPACITY]
            .iter()
            .for_each(|scope| inline.add(*scope));
        assert_eq!(spilled, inline);
        assert_eq!(Scopes::hash_of(&spilled), Scopes::hash_of(&inline));
    }

    #[test]
    fn a_set_shows_its_scopes() {
        let (set, _) = Scopes::new().set_of(2);
        assert_eq!(format!("{set:?}"), "{Scope(0), Scope(1)}");
    }

    #[test]
    fn a_prefix_shares_its_scopes_instead_of_copying_them() {
        let (set, scopes) = Scopes::new().shared_set_of(SortedScopes::INLINE_CAPACITY * 100);
        assert!(set.own().is_empty());
        assert_eq!(set.len(), scopes.len());
    }

    #[test]
    fn a_prefix_keeps_the_scopes_its_chain_held_when_taken() {
        let mut scopes = Scopes::new();
        let (chain, held) = scopes.chain_of(3);
        let mut set = ScopeSet::default();
        set.add_prefix(&Scopes::prefix_of(&chain));
        chain.push(scopes.allocator.fresh());
        assert_eq!(set.iter().collect::<Vec<_>>(), held);
    }

    #[test]
    fn a_shared_set_equals_and_hashes_like_its_scopes_added_one_by_one() {
        let (shared, scopes) = Scopes::new().shared_set_of(Scopes::SHARED_LENGTH);
        let added = Scopes::added_one_by_one(&scopes);
        assert_eq!(shared, added);
        assert_eq!(Scopes::hash_of(&shared), Scopes::hash_of(&added));
        assert_eq!(shared.largest(), added.largest());
        assert!(shared.is_subset_of(&added) && added.is_subset_of(&shared));
    }

    #[test]
    fn own_scopes_and_shared_scopes_come_back_sorted() {
        let mut scopes = Scopes::new();
        let older = scopes.allocator.fresh();
        let (mut set, shared) = scopes.shared_set_of(Scopes::SHARED_LENGTH);
        let newer = scopes.allocator.fresh();
        set.add(newer);
        set.add(older);
        let expected: Vec<Scope> = [older].into_iter().chain(shared).chain([newer]).collect();
        assert_eq!(set.iter().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn a_shorter_prefix_of_a_chain_is_a_subset_of_a_longer_one() {
        let mut scopes = Scopes::new();
        let (chain, _) = scopes.chain_of(Scopes::SHARED_LENGTH);
        let mut shorter = ScopeSet::default();
        shorter.add_prefix(&Scopes::prefix_of(&chain));
        chain.push(scopes.allocator.fresh());
        let mut longer = ScopeSet::default();
        longer.add_prefix(&Scopes::prefix_of(&chain));
        assert!(shorter.is_subset_of(&longer));
        assert!(!longer.is_subset_of(&shorter));
        assert_ne!(shorter, longer);
    }

    #[test]
    fn a_prefix_of_another_chain_is_not_a_subset() {
        let mut scopes = Scopes::new();
        let (first, _) = scopes.shared_set_of(Scopes::SHARED_LENGTH);
        let (second, _) = scopes.shared_set_of(Scopes::SHARED_LENGTH);
        assert!(!second.is_subset_of(&first));
    }

    #[test]
    fn a_scope_between_a_prefixs_scopes_is_not_in_the_set() {
        let mut scopes = Scopes::new();
        let chain = ScopeChain::default();
        chain.push(scopes.allocator.fresh());
        let between = scopes.allocator.fresh();
        (1..Scopes::SHARED_LENGTH).for_each(|_| chain.push(scopes.allocator.fresh()));
        let mut set = ScopeSet::default();
        set.add_prefix(&Scopes::prefix_of(&chain));
        assert!(!set.contains(between));
    }

    #[test]
    fn sets_sharing_prefixes_of_different_chains_differ() {
        let mut scopes = Scopes::new();
        let (first, _) = scopes.shared_set_of(Scopes::SHARED_LENGTH);
        let (second, _) = scopes.shared_set_of(Scopes::SHARED_LENGTH);
        assert_ne!(first, second);
    }

    #[test]
    fn a_set_sharing_two_prefixes_contains_the_scopes_of_each() {
        let mut scopes = Scopes::new();
        let (mut set, older) = scopes.shared_set_of(Scopes::SHARED_LENGTH);
        let (newer_chain, newer) = scopes.chain_of(Scopes::SHARED_LENGTH);
        set.add_prefix(&Scopes::prefix_of(&newer_chain));
        assert!(set.contains(older[0]));
        assert!(set.contains(newer[0]));
    }

    #[test]
    fn a_set_sharing_two_prefixes_has_the_newer_prefix_largest() {
        let mut scopes = Scopes::new();
        let (mut set, _) = scopes.shared_set_of(Scopes::SHARED_LENGTH);
        let (newer_chain, newer) = scopes.chain_of(Scopes::SHARED_LENGTH);
        set.add_prefix(&Scopes::prefix_of(&newer_chain));
        assert_eq!(set.largest(), newer.last().copied());
    }

    #[test]
    fn an_own_scope_beside_a_prefix_keeps_the_set_from_being_a_subset_of_the_prefix() {
        let mut scopes = Scopes::new();
        let (chain, _) = scopes.chain_of(Scopes::SHARED_LENGTH);
        let mut prefix_alone = ScopeSet::default();
        prefix_alone.add_prefix(&Scopes::prefix_of(&chain));
        let mut with_own_scope = prefix_alone.clone();
        with_own_scope.add(scopes.allocator.fresh());
        assert!(!with_own_scope.is_subset_of(&prefix_alone));
    }

    #[test]
    fn adding_a_shared_scope_keeps_the_set() {
        let (mut set, scopes) = Scopes::new().shared_set_of(Scopes::SHARED_LENGTH);
        set.add(scopes[0]);
        assert!(set.own().is_empty());
        assert_eq!(set.len(), scopes.len());
    }

    #[test]
    fn a_prefix_older_than_the_set_adds_its_scopes_one_by_one() {
        let mut scopes = Scopes::new();
        let (chain, shared) = scopes.chain_of(Scopes::SHARED_LENGTH);
        let newer = scopes.allocator.fresh();
        let mut set = Scopes::added_one_by_one(&[newer]);
        set.add_prefix(&Scopes::prefix_of(&chain));
        let expected: Vec<Scope> = shared.into_iter().chain([newer]).collect();
        assert_eq!(set.own(), expected);
    }

    #[test]
    fn removing_an_absent_scope_keeps_the_set() {
        let mut scopes = Scopes::new();
        let (mut set, _) = scopes.set_of(2);
        let original = set.clone();
        let (_, absent) = scopes.set_of(1);
        set.remove(absent[0]);
        assert_eq!(set, original);
    }

    #[test]
    fn removing_a_shared_scope_keeps_the_others() {
        let (mut set, scopes) = Scopes::new().shared_set_of(Scopes::SHARED_LENGTH);
        set.remove(scopes[0]);
        assert_eq!(set.iter().collect::<Vec<_>>(), scopes[1..]);
    }

    #[test]
    fn flipping_a_shared_scope_keeps_the_others() {
        let (mut set, scopes) = Scopes::new().shared_set_of(Scopes::SHARED_LENGTH);
        set.flip(scopes[0]);
        assert_eq!(set.iter().collect::<Vec<_>>(), scopes[1..]);
    }

    #[test]
    fn retaining_keeps_or_drops_a_whole_prefix_without_copying_it() {
        let mut scopes = Scopes::new();
        let (mut kept, _) = scopes.shared_set_of(Scopes::SHARED_LENGTH);
        let (dropped_chain, dropped) = scopes.chain_of(Scopes::SHARED_LENGTH);
        kept.add_prefix(&Scopes::prefix_of(&dropped_chain));
        kept.retain(|scope| !dropped.contains(&scope));
        assert!(kept.own().is_empty());
        assert_eq!(kept.len(), Scopes::SHARED_LENGTH);
    }

    #[test]
    fn retaining_none_of_a_prefix_leaves_the_set_plain() {
        let (mut set, _) = Scopes::new().shared_set_of(Scopes::SHARED_LENGTH);
        set.retain(|_| false);
        assert!(set.is_plain());
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn retaining_part_of_a_prefix_keeps_the_chosen_scopes() {
        let (mut set, mut scopes) = Scopes::new().shared_set_of(Scopes::SHARED_LENGTH);
        let removed = scopes.remove(1);
        set.retain(|scope| scope != removed);
        assert_eq!(set.iter().collect::<Vec<_>>(), scopes);
    }

    #[test]
    fn a_short_prefix_is_added_scope_by_scope() {
        let (set, scopes) = Scopes::new().shared_set_of(ScopeSet::SHORTEST_SHARED_PREFIX - 1);
        assert_eq!(set.own(), scopes);
    }
}
