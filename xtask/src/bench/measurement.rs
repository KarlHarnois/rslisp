use std::collections::BTreeMap;

use rslisp::language::Language;

use super::cost::{Cost, EventLayout};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Measurement {
    stages: BTreeMap<String, Cost>,
    total: Cost,
}

impl Measurement {
    pub const OTHER: &'static str = "other";
    pub const TOTAL: &'static str = "total";

    pub fn new(stages: BTreeMap<String, Cost>, total: Cost) -> Self {
        Self { stages, total }
    }

    pub fn method() -> String {
        format!(
            "Callgrind counts each corpus program under `{name} emit`, which reads, expands, checks, \
             emits and formats the program but never runs `rustc`. \
             Estimated cycles add {first} per first-level cache miss and {last} per last-level miss to the \
             instruction count, on a fixed cache model.\n\n\
             A stage is the inclusive cost of the calls the command-line module makes into one \
             compiler module. `{other}` holds the rest: process startup, argument parsing, \
             reading the entry file, printing, and dropping each stage's result. \
             `rustfmt` runs as a child process that callgrind does not trace, so the `build` stage \
             counts only the compiler's side of formatting.\n\n\
             The compiler runs with a fixed random seed, so its hash maps lay out the same way \
             and identical runs give identical counts, except that the `build` stage can vary \
             by a few hundred instructions with the number of reads from `rustfmt`'s output.\n",
            name = Language::NAME,
            first = EventLayout::FIRST_LEVEL_MISS_CYCLES,
            last = EventLayout::LAST_LEVEL_MISS_CYCLES,
            other = Self::OTHER,
        )
    }

    pub fn modules(&self) -> impl Iterator<Item = &str> {
        self.stages.keys().map(String::as_str)
    }

    pub fn rows(&self) -> impl Iterator<Item = (&str, Cost)> {
        let staged = self.stages.values().copied().sum::<Cost>();
        self.stages
            .iter()
            .map(|(stage, cost)| (stage.as_str(), *cost))
            .chain([
                (Self::OTHER, self.total - staged),
                (Self::TOTAL, self.total),
            ])
    }

    pub fn total(&self) -> Cost {
        self.total
    }

    pub fn cost_of(&self, stage: &str) -> Option<Cost> {
        self.rows()
            .find(|(name, _)| *name == stage)
            .map(|(_, cost)| cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charges_the_unstaged_remainder_to_other() {
        let stages = BTreeMap::from([("emit".to_string(), Cost::new(30, 40))]);
        let measurement = Measurement::new(stages, Cost::new(100, 150));
        assert_eq!(
            measurement.cost_of(Measurement::OTHER),
            Some(Cost::new(70, 110))
        );
        assert_eq!(measurement.cost_of("read"), None);
    }
}
