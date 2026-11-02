use std::collections::BTreeSet;
use std::fmt;

use super::change::Change;
use super::results::{ProgramResult, ResultSet};
use super::table::GroupedCount;
use crate::failure::TaskFailure;

pub struct CounterGate<'a> {
    baseline: &'a ResultSet,
    candidate: &'a ResultSet,
    accepted: &'a BTreeSet<String>,
}

impl<'a> CounterGate<'a> {
    pub const GATED_COUNTERS: [&'static str; 2] =
        ["allocator.allocated_bytes", "allocator.peak_live_bytes"];
    pub const TOLERANCE_PERCENT: u64 = 1;
    pub const NO_BASELINE_COUNTERS: &'static str =
        "The base has no counters, so no counter is checked for growth.\n";

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

    pub fn check(&self) -> Result<String, TaskFailure> {
        if !self.baseline_has_counters() {
            return Ok(Self::NO_BASELINE_COUNTERS.to_string());
        }
        self.require_gated_counters()?;
        let unaccepted: Vec<String> = self
            .rises()
            .iter()
            .filter(|rise| !self.accepts(rise))
            .map(CounterRise::describe)
            .collect();
        match unaccepted.is_empty() {
            true => Ok(format!(
                "No gated counter rose more than {}% without a `Counters-Accepted:` trailer that names its program.\n",
                Self::TOLERANCE_PERCENT
            )),
            false => Err(TaskFailure::counters_rose(
                Self::TOLERANCE_PERCENT,
                &unaccepted,
            )),
        }
    }

    fn require_gated_counters(&self) -> Result<(), TaskFailure> {
        match self.missing_gated_counter() {
            Some(missing) => Err(TaskFailure::gated_counter_missing(&missing)),
            None => Ok(()),
        }
    }

    pub fn missing_gated_counter(&self) -> Option<MissingGatedCounter<'a>> {
        self.candidate.programs().iter().find_map(|candidate| {
            let baseline = self.baseline.find(&candidate.program)?;
            Self::GATED_COUNTERS
                .into_iter()
                .find(|counter| {
                    baseline.counters.contains_key(*counter)
                        && !candidate.counters.contains_key(*counter)
                })
                .map(|counter| MissingGatedCounter {
                    program: &candidate.program,
                    counter,
                })
        })
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

pub struct MissingGatedCounter<'a> {
    program: &'a str,
    counter: &'static str,
}

impl fmt::Display for MissingGatedCounter<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "the candidate has no `{}` for `{}`, so its growth cannot be checked",
            self.counter, self.program
        )
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

        fn check(
            baseline: &ResultSet,
            candidate: &ResultSet,
            accepted: &[&str],
        ) -> Result<String, TaskFailure> {
            let accepted = accepted.iter().map(|program| program.to_string()).collect();
            CounterGate::new(baseline, candidate, &accepted).check()
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

    #[test]
    fn the_check_passes_a_rise_of_exactly_the_tolerance() {
        let checked = GateFixture::check(
            &GateFixture::results("warehouse", 1_000),
            &GateFixture::results("warehouse", 1_010),
            &[],
        );
        assert!(checked.is_ok());
    }

    #[test]
    fn the_check_fails_a_rise_above_the_tolerance() {
        let failure = GateFixture::check(
            &GateFixture::results("warehouse", 1_000),
            &GateFixture::results("warehouse", 1_011),
            &["orders"],
        )
        .unwrap_err();
        assert!(
            failure
                .to_string()
                .contains("- `warehouse` `allocator.allocated_bytes`: 1,000 to 1,011 (+1.10%)")
        );
    }

    #[test]
    fn the_check_passes_an_accepted_rise() {
        let checked = GateFixture::check(
            &GateFixture::results("warehouse", 1_000),
            &GateFixture::results("warehouse", 2_000),
            &["warehouse"],
        );
        assert!(checked.is_ok());
    }

    #[test]
    fn the_check_passes_a_fall() {
        let checked = GateFixture::check(
            &GateFixture::results("warehouse", 1_000),
            &GateFixture::results("warehouse", 500),
            &[],
        );
        assert!(checked.is_ok());
    }

    #[test]
    fn the_check_passes_a_new_program() {
        let checked = GateFixture::check(
            &GateFixture::results("warehouse", 1_000),
            &GateFixture::results("orders", 9_000),
            &[],
        );
        assert!(checked.is_ok());
    }

    #[test]
    fn the_check_passes_a_baseline_without_counters_and_says_so() {
        let checked = GateFixture::check(
            &GateFixture::uncounted("warehouse"),
            &GateFixture::results("warehouse", 9_000),
            &[],
        );
        assert_eq!(checked.unwrap(), CounterGate::NO_BASELINE_COUNTERS);
    }

    #[test]
    fn the_check_fails_a_candidate_without_a_gated_counter_of_the_baseline() {
        let failure = GateFixture::check(
            &GateFixture::results("warehouse", 1_000),
            &GateFixture::uncounted("warehouse"),
            &[],
        )
        .unwrap_err();
        let message = failure.to_string();
        assert!(message.contains("`warehouse`"), "{message}");
        assert!(message.contains("`allocator.allocated_bytes`"), "{message}");
    }
}
