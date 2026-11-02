use std::collections::BTreeSet;

use super::change::Change;
use super::counter_gate::{CounterGate, CounterRise};
use super::results::ResultSet;
use super::table::{GroupedCount, MarkdownTable};

pub struct CounterComparison<'a> {
    baseline: &'a ResultSet,
    candidate: &'a ResultSet,
    gate: CounterGate<'a>,
}

impl<'a> CounterComparison<'a> {
    const HEADER: [&'static str; 5] = ["Program", "Counter", "Before", "After", "Change"];

    pub fn new(
        baseline: &'a ResultSet,
        candidate: &'a ResultSet,
        accepted: &'a BTreeSet<String>,
    ) -> Self {
        Self {
            baseline,
            candidate,
            gate: CounterGate::new(baseline, candidate, accepted),
        }
    }

    pub fn report(&self) -> String {
        format!("{}{}", self.rise_report(), self.every_counter())
    }

    fn rise_report(&self) -> String {
        if !self.gate.baseline_has_counters() {
            return CounterGate::NO_BASELINE_COUNTERS.to_string();
        }
        let rises = self.gate.rises();
        if rises.is_empty() {
            return format!(
                "No program's {} rose more than {}%.\n",
                Self::gated_names(),
                CounterGate::TOLERANCE_PERCENT
            );
        }
        let list: String = rises.iter().map(|rise| self.rise_line(rise)).collect();
        format!(
            "These counters rose more than {}%:\n\n{list}",
            CounterGate::TOLERANCE_PERCENT
        )
    }

    fn gated_names() -> String {
        CounterGate::GATED_COUNTERS
            .map(|counter| format!("`{counter}`"))
            .join(" or ")
    }

    fn rise_line(&self, rise: &CounterRise) -> String {
        let acceptance = match self.gate.accepts(rise) {
            true => "a `Counters-Accepted:` trailer names the program",
            false => "no `Counters-Accepted:` trailer names the program",
        };
        format!("- {}, and {acceptance}\n", rise.describe())
    }

    fn every_counter(&self) -> String {
        let changes = self.changes();
        if changes.is_empty() {
            return String::new();
        }
        let mut table = MarkdownTable::new(&Self::HEADER);
        changes.iter().for_each(|change| table.push(change.row()));
        format!(
            "\n<details><summary>Every counter of every program</summary>\n\n{table}\n</details>\n"
        )
    }

    fn changes(&self) -> Vec<CounterChange<'a>> {
        self.program_names()
            .into_iter()
            .flat_map(|program| self.changes_of(program))
            .collect()
    }

    fn program_names(&self) -> BTreeSet<&'a str> {
        [self.baseline, self.candidate]
            .into_iter()
            .flat_map(ResultSet::programs)
            .map(|result| result.program.as_str())
            .collect()
    }

    fn changes_of(&self, program: &'a str) -> Vec<CounterChange<'a>> {
        let counter_names: BTreeSet<&str> = [self.baseline, self.candidate]
            .into_iter()
            .filter_map(|results| results.find(program))
            .flat_map(|result| result.counters.keys().map(String::as_str))
            .collect();
        counter_names
            .into_iter()
            .map(|counter| CounterChange {
                program,
                counter,
                before: Self::value_of(self.baseline, program, counter),
                after: Self::value_of(self.candidate, program, counter),
            })
            .collect()
    }

    fn value_of(results: &ResultSet, program: &str, counter: &str) -> Option<u64> {
        results.find(program)?.counters.get(counter).copied()
    }
}

struct CounterChange<'a> {
    program: &'a str,
    counter: &'a str,
    before: Option<u64>,
    after: Option<u64>,
}

impl CounterChange<'_> {
    fn row(&self) -> Vec<String> {
        vec![
            self.program.to_string(),
            self.counter.to_string(),
            Self::count(self.before),
            Self::count(self.after),
            self.percent(),
        ]
    }

    fn count(value: Option<u64>) -> String {
        value
            .map(|value| GroupedCount::new(value).to_string())
            .unwrap_or_default()
    }

    fn percent(&self) -> String {
        match (self.before, self.after) {
            (Some(before), Some(after)) => Change::new(before, after).percent(),
            (None, _) => "not counted before".to_string(),
            (_, None) => "not counted after".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::results::{Counters, ProgramResult};

    struct CounterComparisonFixture;

    impl CounterComparisonFixture {
        fn results(counters: &[(&str, &str, u64)]) -> ResultSet {
            let programs: BTreeSet<&str> = counters.iter().map(|(program, ..)| *program).collect();
            let results = programs
                .into_iter()
                .map(|program| ProgramResult {
                    program: program.to_string(),
                    measurement: None,
                    counters: counters
                        .iter()
                        .filter(|(owner, ..)| *owner == program)
                        .map(|(_, counter, value)| (counter.to_string(), *value))
                        .collect::<Counters>(),
                })
                .collect();
            ResultSet::new(results)
        }

        fn report(baseline: &ResultSet, candidate: &ResultSet, accepted: &[&str]) -> String {
            let accepted = accepted.iter().map(|program| program.to_string()).collect();
            CounterComparison::new(baseline, candidate, &accepted).report()
        }
    }

    #[test]
    fn lists_a_rise_named_by_no_trailer() {
        let before =
            CounterComparisonFixture::results(&[("warehouse", "allocator.peak_live_bytes", 1_000)]);
        let after =
            CounterComparisonFixture::results(&[("warehouse", "allocator.peak_live_bytes", 1_100)]);
        let report = CounterComparisonFixture::report(&before, &after, &[]);
        assert!(report.starts_with(
            "These counters rose more than 1%:\n\n\
             - `warehouse` `allocator.peak_live_bytes`: 1,000 to 1,100 (+10.00%), \
             and no `Counters-Accepted:` trailer names the program\n"
        ));
    }

    #[test]
    fn lists_a_rise_that_a_trailer_accepts() {
        let before =
            CounterComparisonFixture::results(&[("warehouse", "allocator.peak_live_bytes", 1_000)]);
        let after =
            CounterComparisonFixture::results(&[("warehouse", "allocator.peak_live_bytes", 1_100)]);
        let report = CounterComparisonFixture::report(&before, &after, &["warehouse"]);
        assert!(report.contains("and a `Counters-Accepted:` trailer names the program\n"));
    }

    #[test]
    fn says_in_one_line_that_no_gated_counter_rose() {
        let before = CounterComparisonFixture::results(&[
            ("warehouse", "allocator.allocated_bytes", 1_000),
            ("warehouse", "emit.emitted_expressions", 10),
        ]);
        let after = CounterComparisonFixture::results(&[
            ("warehouse", "allocator.allocated_bytes", 900),
            ("warehouse", "emit.emitted_expressions", 20),
        ]);
        let report = CounterComparisonFixture::report(&before, &after, &[]);
        assert!(report.starts_with(
            "No program's `allocator.allocated_bytes` or `allocator.peak_live_bytes` rose more than 1%.\n\n<details>"
        ));
    }

    #[test]
    fn tables_every_counter_with_its_change() {
        let before = CounterComparisonFixture::results(&[
            ("warehouse", "allocator.allocated_bytes", 1_000),
            ("warehouse", "allocator.allocations", 4),
        ]);
        let after = CounterComparisonFixture::results(&[
            ("warehouse", "allocator.allocated_bytes", 900),
            ("warehouse", "emit.emitted_expressions", 20),
        ]);
        let report = CounterComparisonFixture::report(&before, &after, &[]);
        assert!(report.contains("| Program | Counter | Before | After | Change |\n"));
        assert!(
            report.contains("| warehouse | allocator.allocated_bytes | 1,000 | 900 | -10.00% |\n")
        );
        assert!(
            report.contains("| warehouse | allocator.allocations | 4 |  | not counted after |\n")
        );
        assert!(
            report.contains(
                "| warehouse | emit.emitted_expressions |  | 20 | not counted before |\n"
            )
        );
    }

    #[test]
    fn says_that_a_base_without_counters_is_not_checked() {
        let before = ResultSet::new(vec![]);
        let after =
            CounterComparisonFixture::results(&[("warehouse", "allocator.allocated_bytes", 900)]);
        let report = CounterComparisonFixture::report(&before, &after, &[]);
        assert!(
            report.starts_with("The base has no counters, so no counter is checked for growth.\n")
        );
    }

    #[test]
    fn leaves_out_the_table_when_nothing_was_counted() {
        let empty = ResultSet::new(vec![]);
        assert_eq!(
            CounterComparisonFixture::report(&empty, &empty, &[]),
            "The base has no counters, so no counter is checked for growth.\n"
        );
    }
}
