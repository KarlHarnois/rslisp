use std::fmt;
use std::hash::{Hash, Hasher};

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
    pub fn add(&mut self, scope: Scope) {
        if let Err(position) = self.position(scope) {
            self.sorted.insert(position, scope);
        }
    }

    pub fn remove(&mut self, scope: Scope) {
        if let Ok(position) = self.position(scope) {
            self.sorted.remove(position);
        }
    }

    pub fn retain(&mut self, keep: impl Fn(Scope) -> bool) {
        self.sorted.retain(keep);
    }

    pub fn flip(&mut self, scope: Scope) {
        match self.position(scope) {
            Ok(position) => self.sorted.remove(position),
            Err(position) => self.sorted.insert(position, scope),
        }
    }

    pub fn contains(&self, scope: Scope) -> bool {
        self.position(scope).is_ok()
    }

    #[inline]
    pub fn is_subset_of(&self, other: &ScopeSet) -> bool {
        let mut larger = other.iter();
        self.iter()
            .all(|scope| larger.any(|candidate| candidate == scope))
    }

    pub(super) fn largest(&self) -> Option<Scope> {
        self.sorted.as_slice().last().copied()
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = Scope> {
        self.sorted.as_slice().iter().copied()
    }

    pub(super) fn len(&self) -> usize {
        self.sorted.as_slice().len()
    }

    fn position(&self, scope: Scope) -> Result<usize, usize> {
        self.sorted.as_slice().binary_search(&scope)
    }
}

impl PartialEq for ScopeSet {
    fn eq(&self, other: &Self) -> bool {
        self.sorted.as_slice() == other.sorted.as_slice()
    }
}

impl Eq for ScopeSet {}

impl Hash for ScopeSet {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.sorted.as_slice().hash(state);
    }
}

impl fmt::Debug for ScopeSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_set().entries(self.iter()).finish()
    }
}

#[derive(Clone)]
enum SortedScopes {
    Inline {
        length: usize,
        scopes: [Scope; SortedScopes::INLINE_CAPACITY],
    },
    Spilled(Vec<Scope>),
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

    fn as_slice(&self) -> &[Scope] {
        match self {
            Self::Inline { length, scopes } => &scopes[..*length],
            Self::Spilled(scopes) => scopes,
        }
    }

    fn insert(&mut self, position: usize, scope: Scope) {
        match self {
            Self::Inline { length, scopes } if *length < Self::INLINE_CAPACITY => {
                scopes.copy_within(position..*length, position + 1);
                scopes[position] = scope;
                *length += 1;
            }
            Self::Inline { .. } => *self = Self::Spilled(self.spilled_with(position, scope)),
            Self::Spilled(scopes) => scopes.insert(position, scope),
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
            Self::Spilled(scopes) => {
                scopes.remove(position);
            }
        }
    }

    fn retain(&mut self, keep: impl Fn(Scope) -> bool) {
        match self {
            Self::Inline { length, scopes } => {
                *length = Self::retain_inline(&mut scopes[..*length], keep)
            }
            Self::Spilled(scopes) => scopes.retain(|scope| keep(*scope)),
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

    struct Scopes {
        allocator: ScopeAllocator,
    }

    impl Scopes {
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
}
