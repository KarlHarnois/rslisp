use std::collections::BTreeMap;

use rslisp::language::Language;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Measurement {
    stages: BTreeMap<String, u64>,
    total: u64,
}

impl Measurement {
    pub const OTHER: &'static str = "other";
    pub const TOTAL: &'static str = "total";

    pub fn new(stages: BTreeMap<String, u64>, total: u64) -> Self {
        Self { stages, total }
    }

    pub fn method() -> String {
        format!(
            "Callgrind counts each corpus program under `{name} emit`, which reads, expands, checks, \
             emits and formats the program but never runs `rustc`.\n\n\
             A stage is the inclusive cost of the calls the command-line module makes into one \
             compiler module. `{other}` holds the rest: process startup, argument parsing, \
             reading the entry file, printing, and dropping each stage's result. \
             `rustfmt` runs as a child process that callgrind does not trace, so the `build` stage \
             counts only the compiler's side of formatting.\n\n\
             `empty_program` holds only an empty `main`, so its counts are the cost that every \
             program pays, such as expanding and checking the prelude.\n\n\
             The compiler runs with a fixed random seed, so its hash maps lay out the same way \
             and identical runs give identical counts, except that the `build` stage can vary \
             by a few hundred instructions with the number of reads from `rustfmt`'s output.\n",
            name = Language::NAME,
            other = Self::OTHER,
        )
    }

    pub fn modules(&self) -> impl Iterator<Item = &str> {
        self.stages.keys().map(String::as_str)
    }

    pub fn rows(&self) -> impl Iterator<Item = (&str, u64)> {
        let staged: u64 = self.stages.values().sum();
        self.stages
            .iter()
            .map(|(stage, instructions)| (stage.as_str(), *instructions))
            .chain([
                (Self::OTHER, self.total.saturating_sub(staged)),
                (Self::TOTAL, self.total),
            ])
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn instructions_of(&self, stage: &str) -> Option<u64> {
        self.rows()
            .find(|(name, _)| *name == stage)
            .map(|(_, instructions)| instructions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charges_the_unstaged_remainder_to_other() {
        let stages = BTreeMap::from([("emit".to_string(), 30)]);
        let measurement = Measurement::new(stages, 100);
        assert_eq!(measurement.instructions_of(Measurement::OTHER), Some(70));
        assert_eq!(measurement.instructions_of("read"), None);
    }
}
