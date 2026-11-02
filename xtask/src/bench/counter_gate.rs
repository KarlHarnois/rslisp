use std::collections::BTreeSet;

use super::change::Change;
use super::results::{ProgramResult, ResultSet};
use super::table::GroupedCount;

pub struct CounterGate<'a> {
    baseline: &'a ResultSet,
    candidate: &'a ResultSet,
    accepted: &'a BTreeSet<String>,
}

impl<'a> CounterGate<'a> {
    pub const GATED_COUNTERS: [&'static str; 2] =
        ["allocator.allocated_bytes", "allocator.peak_live_bytes"];
    pub const TOLERANCE_PERCENT: u64 = 1;

    pub fn new(
        baseline: &'a ResultSet,
        candidate: &'a ResultSet,
        accepted: &'a BTreeSet<String>,
    ) -> Self {
        Self {
            baseline,
            candidate,
            accepted,
        }
    }

    pub fn baseline_has_counters(&self) -> bool {
        self.baseline
            .programs()
            .iter()
            .any(|result| !result.counters.is_empty())
    }

    pub fn rises(&self) -> Vec<CounterRise<'a>> {
        self.candidate
            .programs()
            .iter()
            .flat_map(|candidate| self.rises_of(candidate))
            .collect()
    }

    pub fn accepts(&self, rise: &CounterRise) -> bool {
        self.accepted.contains(rise.program)
    }

    fn rises_of(&self, candidate: &'a ProgramResult) -> Vec<CounterRise<'a>> {
        let Some(baseline) = self.baseline.find(&candidate.program) else {
            return Vec::new();
        };
        Self::GATED_COUNTERS
            .into_iter()
            .filter_map(|counter| CounterRise::of(counter, baseline, candidate))
            .filter(|rise| rise.exceeds_tolerance())
            .collect()
    }
}

pub struct CounterRise<'a> {
    program: &'a str,
    counter: &'static str,
    before: u64,
    after: u64,
}

impl<'a> CounterRise<'a> {
    fn of(
        counter: &'static str,
        baseline: &ProgramResult,
        candidate: &'a ProgramResult,
    ) -> Option<Self> {
        Some(Self {
            program: &candidate.program,
            counter,
            before: *baseline.counters.get(counter)?,
            after: *candidate.counters.get(counter)?,
        })
    }

    fn exceeds_tolerance(&self) -> bool {
        self.change()
            .rises_more_than_percent(CounterGate::TOLERANCE_PERCENT)
    }

    pub fn describe(&self) -> String {
        format!(
            "`{}` `{}`: {} to {} ({})",
            self.program,
            self.counter,
            GroupedCount::new(self.before),
            GroupedCount::new(self.after),
            self.change().percent()
        )
    }

    fn change(&self) -> Change {
        Change::new(self.before, self.after)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::results::Counters;

    struct GateFixture;

    impl GateFixture {
        const ALLOCATED_BYTES: &'static str = "allocator.allocated_bytes";

        fn results(program: &str, allocated_bytes: u64) -> ResultSet {
            ResultSet::new(vec![Self::program(
                program,
                Counters::from([(Self::ALLOCATED_BYTES.to_string(), allocated_bytes)]),
            )])
        }

        fn uncounted(program: &str) -> ResultSet {
            ResultSet::new(vec![Self::program(program, Counters::new())])
        }

        fn program(program: &str, counters: Counters) -> ProgramResult {
            ProgramResult {
                program: program.to_string(),
                measurement: None,
                counters,
            }
        }

        fn rises(baseline: &ResultSet, candidate: &ResultSet) -> Vec<String> {
            CounterGate::new(baseline, candidate, &BTreeSet::new())
                .rises()
                .iter()
                .map(CounterRise::describe)
                .collect()
        }
    }

    #[test]
    fn a_rise_of_exactly_the_tolerance_passes() {
        let rises = GateFixture::rises(
            &GateFixture::results("warehouse", 1_000),
            &GateFixture::results("warehouse", 1_010),
        );
        assert!(rises.is_empty());
    }

    #[test]
    fn a_rise_above_the_tolerance_is_a_rise() {
        let rises = GateFixture::rises(
            &GateFixture::results("warehouse", 1_000),
            &GateFixture::results("warehouse", 1_011),
        );
        assert_eq!(
            rises,
            ["`warehouse` `allocator.allocated_bytes`: 1,000 to 1,011 (+1.10%)"]
        );
    }

    #[test]
    fn a_program_missing_from_the_baseline_has_no_rise() {
        let rises = GateFixture::rises(
            &GateFixture::results("warehouse", 1_000),
            &GateFixture::results("orders", 5_000),
        );
        assert!(rises.is_empty());
    }

    #[test]
    fn a_counter_missing_from_the_baseline_has_no_rise() {
        let rises = GateFixture::rises(
            &GateFixture::uncounted("warehouse"),
            &GateFixture::results("warehouse", 5_000),
        );
        assert!(rises.is_empty());
    }

    #[test]
    fn knows_a_baseline_without_counters() {
        let baseline = GateFixture::uncounted("warehouse");
        let candidate = GateFixture::results("warehouse", 5_000);
        let accepted = BTreeSet::new();
        let gate = CounterGate::new(&baseline, &candidate, &accepted);
        assert!(!gate.baseline_has_counters());
        assert!(CounterGate::new(&candidate, &baseline, &accepted).baseline_has_counters());
    }
}
