use std::iter::Sum;
use std::ops::{Add, AddAssign, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cost {
    pub instructions: u64,
    pub estimated_cycles: u64,
}

impl Cost {
    pub fn new(instructions: u64, estimated_cycles: u64) -> Self {
        Self {
            instructions,
            estimated_cycles,
        }
    }
}

impl Add for Cost {
    type Output = Cost;

    fn add(self, other: Cost) -> Cost {
        Cost {
            instructions: self.instructions + other.instructions,
            estimated_cycles: self.estimated_cycles + other.estimated_cycles,
        }
    }
}

impl AddAssign for Cost {
    fn add_assign(&mut self, other: Cost) {
        *self = *self + other;
    }
}

impl Sub for Cost {
    type Output = Cost;

    fn sub(self, other: Cost) -> Cost {
        Cost {
            instructions: self.instructions.saturating_sub(other.instructions),
            estimated_cycles: self.estimated_cycles.saturating_sub(other.estimated_cycles),
        }
    }
}

impl Sum for Cost {
    fn sum<I: Iterator<Item = Cost>>(costs: I) -> Cost {
        costs.fold(Cost::default(), Add::add)
    }
}

pub struct EventLayout {
    names: Vec<String>,
}

impl EventLayout {
    const INSTRUCTIONS: &'static str = "Ir";
    const FIRST_LEVEL_MISSES: [&'static str; 3] = ["I1mr", "D1mr", "D1mw"];
    const LAST_LEVEL_MISSES: [&'static str; 3] = ["ILmr", "DLmr", "DLmw"];
    pub const FIRST_LEVEL_MISS_CYCLES: u64 = 10;
    pub const LAST_LEVEL_MISS_CYCLES: u64 = 100;

    pub fn new(header: &str) -> Self {
        Self {
            names: header.split_whitespace().map(str::to_string).collect(),
        }
    }

    pub fn cost(&self, counts: &[u64]) -> Cost {
        let instructions = self.count(counts, Self::INSTRUCTIONS);
        let first_level_misses = self.total(counts, &Self::FIRST_LEVEL_MISSES);
        let last_level_misses = self.total(counts, &Self::LAST_LEVEL_MISSES);
        Cost {
            instructions,
            estimated_cycles: instructions
                + Self::FIRST_LEVEL_MISS_CYCLES * first_level_misses
                + Self::LAST_LEVEL_MISS_CYCLES * last_level_misses,
        }
    }

    fn total(&self, counts: &[u64], events: &[&str]) -> u64 {
        events.iter().map(|event| self.count(counts, event)).sum()
    }

    fn count(&self, counts: &[u64], event: &str) -> u64 {
        self.names
            .iter()
            .position(|name| name == event)
            .and_then(|index| counts.get(index))
            .copied()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighs_cache_misses_into_estimated_cycles() {
        let layout = EventLayout::new("Ir Dr Dw I1mr D1mr D1mw ILmr DLmr DLmw");
        let cost = layout.cost(&[1000, 0, 0, 1, 2, 3, 1, 0, 1]);
        assert_eq!(cost.instructions, 1000);
        assert_eq!(cost.estimated_cycles, 1000 + 10 * 6 + 100 * 2);
    }

    #[test]
    fn treats_omitted_trailing_counts_as_zero() {
        let layout = EventLayout::new("Ir Dr Dw I1mr D1mr D1mw ILmr DLmr DLmw");
        assert_eq!(layout.cost(&[7]).estimated_cycles, 7);
    }
}
