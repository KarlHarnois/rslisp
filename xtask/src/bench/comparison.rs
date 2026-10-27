use std::collections::BTreeSet;
use std::fmt;

use super::cost::Cost;
use super::measurement::Measurement;
use super::results::{ProgramResult, ResultSet};
use super::table::{GroupedCount, MarkdownTable};

pub struct Comparison<'a> {
    baseline: &'a ResultSet,
    candidate: &'a ResultSet,
}

impl<'a> Comparison<'a> {
    const HEADER: [&'static str; 8] = [
        "Program",
        "Stage",
        "Instructions before",
        "Instructions after",
        "Change",
        "Estimated cycles before",
        "Estimated cycles after",
        "Change",
    ];

    pub fn new(baseline: &'a ResultSet, candidate: &'a ResultSet) -> Self {
        Self {
            baseline,
            candidate,
        }
    }

    fn programs(&self) -> Vec<ProgramComparison<'a>> {
        self.program_names()
            .into_iter()
            .map(|program| self.compare_program(program))
            .collect()
    }

    fn program_names(&self) -> BTreeSet<&'a str> {
        [self.baseline, self.candidate]
            .into_iter()
            .flat_map(ResultSet::programs)
            .map(|result| result.program.as_str())
            .collect()
    }

    fn compare_program(&self, program: &'a str) -> ProgramComparison<'a> {
        let measured = |results: &'a ResultSet| {
            results
                .find(program)
                .and_then(|result: &'a ProgramResult| result.measurement.as_ref())
        };
        match (measured(self.baseline), measured(self.candidate)) {
            (Some(before), Some(after)) => {
                ProgramComparison::Measured(StageChange::all(program, before, after))
            }
            (None, _) => ProgramComparison::Unmeasured {
                program,
                side: "before",
            },
            (_, None) => ProgramComparison::Unmeasured {
                program,
                side: "after",
            },
        }
    }

    fn write_summary(
        formatter: &mut fmt::Formatter<'_>,
        programs: &[ProgramComparison],
    ) -> fmt::Result {
        let notes: Vec<String> = programs.iter().flat_map(ProgramComparison::notes).collect();
        if notes.is_empty() {
            return writeln!(
                formatter,
                "No stage of any program rose by more than {}%.",
                Change::NOTABLE_RISE_PERCENT
            );
        }
        writeln!(
            formatter,
            "These stages rose by more than {}%, or could not be compared:\n",
            Change::NOTABLE_RISE_PERCENT
        )?;
        notes
            .iter()
            .try_for_each(|note| writeln!(formatter, "- {note}"))
    }

    fn table(programs: &[ProgramComparison]) -> MarkdownTable {
        let mut table = MarkdownTable::new(&Self::HEADER);
        programs
            .iter()
            .flat_map(ProgramComparison::rows)
            .for_each(|row| table.push(row));
        table
    }
}

impl fmt::Display for Comparison<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let programs = self.programs();
        Self::write_summary(formatter, &programs)?;
        write!(
            formatter,
            "\n<details><summary>Every stage of every program</summary>\n\n{}\n</details>\n\n{}",
            Self::table(&programs),
            Measurement::method()
        )
    }
}

enum ProgramComparison<'a> {
    Measured(Vec<StageChange<'a>>),
    Unmeasured {
        program: &'a str,
        side: &'static str,
    },
}

impl ProgramComparison<'_> {
    fn notes(&self) -> Vec<String> {
        match self {
            Self::Measured(stages) => stages
                .iter()
                .filter(|stage| stage.rises_notably())
                .map(StageChange::note)
                .collect(),
            Self::Unmeasured { program, side } => {
                vec![format!("`{program}` was not measured {side}")]
            }
        }
    }

    fn rows(&self) -> Vec<Vec<String>> {
        match self {
            Self::Measured(stages) => stages.iter().map(StageChange::row).collect(),
            Self::Unmeasured { program, side } => {
                let mut row = vec![program.to_string(), format!("not measured {side}")];
                row.resize(Comparison::HEADER.len(), String::new());
                vec![row]
            }
        }
    }
}

struct StageChange<'a> {
    program: &'a str,
    stage: &'a str,
    instructions: Change,
    estimated_cycles: Change,
}

impl<'a> StageChange<'a> {
    fn all(program: &'a str, before: &'a Measurement, after: &'a Measurement) -> Vec<Self> {
        let modules: BTreeSet<&str> = before.modules().chain(after.modules()).collect();
        modules
            .into_iter()
            .chain([Measurement::OTHER, Measurement::TOTAL])
            .map(|stage| Self::new(program, stage, before.cost_of(stage), after.cost_of(stage)))
            .collect()
    }

    fn new(program: &'a str, stage: &'a str, before: Option<Cost>, after: Option<Cost>) -> Self {
        let before = before.unwrap_or_default();
        let after = after.unwrap_or_default();
        Self {
            program,
            stage,
            instructions: Change::new(before.instructions, after.instructions),
            estimated_cycles: Change::new(before.estimated_cycles, after.estimated_cycles),
        }
    }

    fn rises_notably(&self) -> bool {
        self.instructions.rises_notably() || self.estimated_cycles.rises_notably()
    }

    fn note(&self) -> String {
        format!(
            "`{}` `{}`: instructions {}, estimated cycles {}",
            self.program,
            self.stage,
            self.instructions.percent(),
            self.estimated_cycles.percent()
        )
    }

    fn row(&self) -> Vec<String> {
        let mut row = vec![self.program.to_string(), self.stage.to_string()];
        row.extend(self.instructions.cells());
        row.extend(self.estimated_cycles.cells());
        row
    }
}

struct Change {
    before: u64,
    after: u64,
}

impl Change {
    const NOTABLE_RISE_PERCENT: f64 = 1.0;
    const PERCENT: f64 = 100.0;

    fn new(before: u64, after: u64) -> Self {
        Self { before, after }
    }

    fn cells(&self) -> [String; 3] {
        let percent = self.percent();
        let emphasized = match self.rises_notably() {
            true => format!("**{percent}**"),
            false => percent,
        };
        [
            GroupedCount::new(self.before).to_string(),
            GroupedCount::new(self.after).to_string(),
            emphasized,
        ]
    }

    fn rises_notably(&self) -> bool {
        self.ratio()
            .is_some_and(|percent| percent > Self::NOTABLE_RISE_PERCENT)
    }

    fn percent(&self) -> String {
        match self.ratio() {
            Some(percent) => format!("{percent:+.2}%"),
            None if self.after == 0 => "0.00%".to_string(),
            None => "new".to_string(),
        }
    }

    fn ratio(&self) -> Option<f64> {
        (self.before > 0)
            .then(|| (self.after as f64 - self.before as f64) / self.before as f64 * Self::PERCENT)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    struct ComparisonFixture;

    impl ComparisonFixture {
        fn results(expand: u64, total: u64) -> ResultSet {
            let stages = BTreeMap::from([("expand".to_string(), Cost::new(expand, expand))]);
            ResultSet::new(vec![
                ProgramResult {
                    program: "inventory".to_string(),
                    measurement: Some(Measurement::new(stages, Cost::new(total, total))),
                },
                ProgramResult {
                    program: "orders".to_string(),
                    measurement: None,
                },
            ])
        }

        fn report(before: &ResultSet, after: &ResultSet) -> String {
            Comparison::new(before, after).to_string()
        }
    }

    #[test]
    fn lists_each_stage_that_rose_by_more_than_one_percent() {
        let before = ComparisonFixture::results(1000, 2000);
        let after = ComparisonFixture::results(1020, 2010);
        let report = ComparisonFixture::report(&before, &after);
        assert!(report.starts_with(
            "These stages rose by more than 1%, or could not be compared:\n\n\
             - `inventory` `expand`: instructions +2.00%, estimated cycles +2.00%\n\
             - `orders` was not measured before\n"
        ));
    }

    #[test]
    fn bolds_a_notable_rise_in_the_table() {
        let before = ComparisonFixture::results(1000, 2000);
        let after = ComparisonFixture::results(1020, 2010);
        let report = ComparisonFixture::report(&before, &after);
        assert!(report.contains(
            "| inventory | expand | 1,000 | 1,020 | **+2.00%** | 1,000 | 1,020 | **+2.00%** |"
        ));
        assert!(report.contains("| inventory | total | 2,000 | 2,010 | +0.50% |"));
        assert!(report.contains("| orders | not measured before |  |  |  |  |  |  |"));
    }

    #[test]
    fn reports_no_rise_when_every_stage_held() {
        let results = ResultSet::new(vec![]);
        let report = ComparisonFixture::report(&results, &results);
        assert!(report.starts_with("No stage of any program rose by more than 1%."));
    }

    #[test]
    fn describes_a_change_from_zero() {
        assert_eq!(Change::new(0, 0).percent(), "0.00%");
        assert_eq!(Change::new(0, 5).percent(), "new");
        assert_eq!(Change::new(200, 100).percent(), "-50.00%");
    }
}
