use std::collections::BTreeSet;

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

    const LARGEST_CHANGES: usize = 20;

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

    pub fn full_report(&self) -> String {
        let programs = self.programs();
        let rows = programs.iter().flat_map(ProgramComparison::rows);
        format!(
            "{}\n<details><summary>Every stage of every program</summary>\n\n{}\n</details>\n\n{}",
            Self::list_notes(&programs),
            Self::table(rows),
            Measurement::method()
        )
    }

    pub fn brief_report(&self) -> String {
        let programs = self.programs();
        let changed = Self::changed_stages(&programs);
        if changed.is_empty() && Self::note_count(&programs) == 0 {
            return "No count changed.\n".to_string();
        }
        format!(
            "{}\n{}",
            Self::count_notes(&programs),
            Self::largest_changes(changed)
        )
    }

    fn list_notes(programs: &[ProgramComparison]) -> String {
        let notes: Vec<String> = programs.iter().flat_map(ProgramComparison::notes).collect();
        if notes.is_empty() {
            return Self::no_notable_rise();
        }
        let list: String = notes.iter().map(|note| format!("- {note}\n")).collect();
        format!(
            "These stages rose by more than {}%, or could not be compared:\n\n{list}",
            Change::NOTABLE_RISE_PERCENT
        )
    }

    fn note_count(programs: &[ProgramComparison]) -> usize {
        programs.iter().map(|program| program.notes().len()).sum()
    }

    fn count_notes(programs: &[ProgramComparison]) -> String {
        let count = Self::note_count(programs);
        if count == 0 {
            return Self::no_notable_rise();
        }
        format!(
            "Stages that rose by more than {}%, or could not be compared: {count}.\n",
            Change::NOTABLE_RISE_PERCENT
        )
    }

    fn no_notable_rise() -> String {
        format!(
            "No stage of any program rose by more than {}%.\n",
            Change::NOTABLE_RISE_PERCENT
        )
    }

    fn changed_stages<'report>(
        programs: &'report [ProgramComparison<'report>],
    ) -> Vec<&'report StageChange<'report>> {
        let mut changed: Vec<&StageChange> = programs
            .iter()
            .flat_map(ProgramComparison::stages)
            .filter(|stage| stage.magnitude() > 0.0)
            .collect();
        changed.sort_by(|left, right| right.magnitude().total_cmp(&left.magnitude()));
        changed
    }

    fn largest_changes(changed: Vec<&StageChange>) -> String {
        if changed.is_empty() {
            return String::new();
        }
        let shown = changed.len().min(Self::LARGEST_CHANGES);
        let rows = changed.iter().take(shown).map(|stage| stage.row());
        format!(
            "The {shown} largest of {} changes:\n\n{}",
            changed.len(),
            Self::table(rows)
        )
    }

    fn table(rows: impl Iterator<Item = Vec<String>>) -> MarkdownTable {
        let mut table = MarkdownTable::new(&Self::HEADER);
        rows.for_each(|row| table.push(row));
        table
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

    fn stages(&self) -> &[StageChange<'_>] {
        match self {
            Self::Measured(stages) => stages,
            Self::Unmeasured { .. } => &[],
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

    fn magnitude(&self) -> f64 {
        self.instructions
            .magnitude()
            .max(self.estimated_cycles.magnitude())
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

    fn magnitude(&self) -> f64 {
        match self.ratio() {
            Some(percent) => percent.abs(),
            None if self.after == 0 => 0.0,
            None => f64::INFINITY,
        }
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
            let orders = ProgramResult {
                program: "orders".to_string(),
                measurement: None,
            };
            ResultSet::new(vec![Self::inventory(expand, total), orders])
        }

        fn measured_only(expand: u64, total: u64) -> ResultSet {
            ResultSet::new(vec![Self::inventory(expand, total)])
        }

        fn inventory(expand: u64, total: u64) -> ProgramResult {
            let stages = BTreeMap::from([("expand".to_string(), Cost::new(expand, expand))]);
            ProgramResult {
                program: "inventory".to_string(),
                measurement: Some(Measurement::new(stages, Cost::new(total, total))),
            }
        }

        fn report(before: &ResultSet, after: &ResultSet) -> String {
            Comparison::new(before, after).full_report()
        }

        fn brief(before: &ResultSet, after: &ResultSet) -> String {
            Comparison::new(before, after).brief_report()
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
    fn brief_report_counts_the_notes_and_ranks_the_changes() {
        let before = ComparisonFixture::results(1000, 2000);
        let after = ComparisonFixture::results(1020, 2010);
        let report = ComparisonFixture::brief(&before, &after);
        assert!(report.starts_with(
            "Stages that rose by more than 1%, or could not be compared: 2.\n\n\
             The 3 largest of 3 changes:\n"
        ));
        let expand = report.find("| inventory | expand |").unwrap();
        let total = report.find("| inventory | total |").unwrap();
        assert!(expand < total);
        assert!(!report.contains("orders"));
    }

    #[test]
    fn brief_report_says_when_no_count_changed() {
        let results = ComparisonFixture::results(1000, 2000);
        let report = ComparisonFixture::brief(&results, &results);
        assert_eq!(
            report,
            "Stages that rose by more than 1%, or could not be compared: 1.\n\n"
        );
        let measured = ComparisonFixture::measured_only(1000, 2000);
        assert_eq!(
            ComparisonFixture::brief(&measured, &measured),
            "No count changed.\n"
        );
    }

    #[test]
    fn describes_a_change_from_zero() {
        assert_eq!(Change::new(0, 0).percent(), "0.00%");
        assert_eq!(Change::new(0, 5).percent(), "new");
        assert_eq!(Change::new(200, 100).percent(), "-50.00%");
    }
}
