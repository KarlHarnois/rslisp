use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

pub type FastHashMap<K, V> = HashMap<K, V, BuildHasherDefault<FastHasher>>;
pub type FastHashSet<T> = HashSet<T, BuildHasherDefault<FastHasher>>;

#[derive(Default)]
pub struct FastHasher {
    hash: u64,
}

impl FastHasher {
    const MULTIPLIER: u64 = 0x517c_c1b7_2722_0a95;
    const ROTATION: u32 = 5;
    const WORD_BYTES: usize = 8;

    fn add_word(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(Self::ROTATION) ^ word).wrapping_mul(Self::MULTIPLIER);
    }

    fn word_of(bytes: &[u8]) -> u64 {
        let mut word = [0; Self::WORD_BYTES];
        word[..bytes.len()].copy_from_slice(bytes);
        u64::from_le_bytes(word)
    }
}

impl Hasher for FastHasher {
    fn write(&mut self, bytes: &[u8]) {
        let words = bytes.chunks_exact(Self::WORD_BYTES);
        let remainder = words.remainder();
        words.for_each(|word| self.add_word(Self::word_of(word)));
        if !remainder.is_empty() {
            self.add_word(Self::word_of(remainder));
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.add_word(u64::from(value));
    }

    fn write_u32(&mut self, value: u32) {
        self.add_word(u64::from(value));
    }

    fn write_u64(&mut self, value: u64) {
        self.add_word(value);
    }

    fn write_usize(&mut self, value: usize) {
        self.add_word(value as u64);
    }

    fn finish(&self) -> u64 {
        self.hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Hashes;

    impl Hashes {
        fn of(value: impl std::hash::Hash) -> u64 {
            let mut hasher = FastHasher::default();
            value.hash(&mut hasher);
            hasher.finish()
        }
    }

    #[test]
    fn equal_names_hash_alike() {
        assert_eq!(Hashes::of("assoc-in"), Hashes::of(String::from("assoc-in")));
    }

    #[test]
    fn names_that_differ_past_the_first_word_hash_apart() {
        assert_ne!(Hashes::of("syntax/defn"), Hashes::of("syntax/defrule"));
    }

    #[test]
    fn numbers_of_each_width_hash_apart() {
        assert_ne!(Hashes::of(1u8), Hashes::of(2u8));
        assert_ne!(Hashes::of(1u32), Hashes::of(2u32));
        assert_ne!(Hashes::of(1u64), Hashes::of(2u64));
        assert_ne!(Hashes::of(1usize), Hashes::of(2usize));
    }
}
