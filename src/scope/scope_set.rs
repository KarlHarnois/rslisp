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

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ScopeSet {
    sorted: Vec<Scope>,
}

impl ScopeSet {
    pub fn add(&mut self, scope: Scope) {
        if let Err(position) = self.sorted.binary_search(&scope) {
            self.sorted.insert(position, scope);
        }
    }

    pub fn remove(&mut self, scope: Scope) {
        if let Ok(position) = self.sorted.binary_search(&scope) {
            self.sorted.remove(position);
        }
    }

    pub fn retain(&mut self, keep: impl Fn(Scope) -> bool) {
        self.sorted.retain(|scope| keep(*scope));
    }

    pub fn flip(&mut self, scope: Scope) {
        match self.sorted.binary_search(&scope) {
            Ok(position) => {
                self.sorted.remove(position);
            }
            Err(position) => self.sorted.insert(position, scope),
        }
    }

    pub fn contains(&self, scope: Scope) -> bool {
        self.sorted.binary_search(&scope).is_ok()
    }

    pub fn is_subset_of(&self, other: &ScopeSet) -> bool {
        self.sorted.iter().all(|scope| other.contains(*scope))
    }

    pub fn len(&self) -> usize {
        self.sorted.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sorted.is_empty()
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
}
