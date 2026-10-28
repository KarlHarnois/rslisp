use std::collections::BTreeSet;

use super::measurement::Measurement;
use super::results::{ProgramResult, ResultSet};
use super::table::{GroupedCount, MarkdownTable};

pub struct Comparison<'a> {
    baseline: &'a ResultSet,
    candidate: &'a ResultSet,
}

impl<'a> Comparison<'a> {
    const HEADER: [&'static str; 5] = [
        "Program",
        "Stage",
        "Instructions before",
        "Instructions after",
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
            "{}\n<details><summary>Every stage of every program</summary>\n\n{}\n</details>\n\n{}\n\n{}",
            Self::list_notes(&programs),
            Self::table(rows),
            Self::criterion(),
            Measurement::method()
        )
    }

    pub fn brief_report(&self) -> String {
        let programs = self.programs();
        let changed = Self::changed_stages(&programs);
        if changed.is_empty() && Self::note_count(&programs) == 0 {
            return "No instruction count changed.\n".to_string();
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
        format!("These stages rose notably, or could not be compared:\n\n{list}")
    }

    fn note_count(programs: &[ProgramComparison]) -> usize {
        programs.iter().map(|program| program.notes().len()).sum()
    }

    fn count_notes(programs: &[ProgramComparison]) -> String {
        let count = Self::note_count(programs);
        if count == 0 {
            return Self::no_notable_rise();
        }
        format!("Stages that rose notably, or could not be compared: {count}.\n")
    }

    fn no_notable_rise() -> String {
        "No stage of any program rose notably.\n".to_string()
    }

    fn criterion() -> String {
        format!(
            "A stage rises notably when its instruction count grows by more than {}% and by at \
             least {}% of its program's instructions.",
            Change::NOTABLE_RISE_PERCENT,
            StageChange::MINIMUM_NOTABLE_SHARE * Change::PERCENT
        )
    }

    fn changed_stages<'report>(
        programs: &'report [ProgramComparison<'report>],
    ) -> Vec<&'report StageChange<'report>> {
        let mut changed: Vec<&StageChange> = programs
            .iter()
            .flat_map(ProgramComparison::stages)
            .filter(|stage| stage.instructions.differs())
            .collect();
        changed.sort_by(|left, right| right.share().total_cmp(&left.share()));
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
    program_instructions: u64,
    instructions: Change,
}

impl<'a> StageChange<'a> {
    const MINIMUM_NOTABLE_SHARE: f64 = 0.001;

    fn all(program: &'a str, before: &'a Measurement, after: &'a Measurement) -> Vec<Self> {
        let modules: BTreeSet<&str> = before.modules().chain(after.modules()).collect();
        let program_instructions = before.total();
        modules
            .into_iter()
            .chain([Measurement::OTHER, Measurement::TOTAL])
            .map(|stage| Self {
                program,
                stage,
                program_instructions,
                instructions: Change::of(before, after, stage),
            })
            .collect()
    }

    fn rises_notably(&self) -> bool {
        self.instructions.exceeds_notable_rise() && self.share() >= Self::MINIMUM_NOTABLE_SHARE
    }

    fn share(&self) -> f64 {
        self.instructions.difference().abs() / self.program_instructions.max(1) as f64
    }

    fn note(&self) -> String {
        format!(
            "`{}` `{}`: {}",
            self.program,
            self.stage,
            self.instructions.percent()
        )
    }

    fn row(&self) -> Vec<String> {
        let instruction_change = match self.rises_notably() {
            true => format!("**{}**", self.instructions.percent()),
            false => self.instructions.percent(),
        };
        let mut row = vec![self.program.to_string(), self.stage.to_string()];
        row.extend(self.instructions.counts());
        row.push(instruction_change);
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

    fn of(before: &Measurement, after: &Measurement, stage: &str) -> Self {
        let count_of = |measurement: &Measurement| measurement.instructions_of(stage).unwrap_or(0);
        Self::new(count_of(before), count_of(after))
    }

    fn counts(&self) -> [String; 2] {
        [
            GroupedCount::new(self.before).to_string(),
            GroupedCount::new(self.after).to_string(),
        ]
    }

    fn exceeds_notable_rise(&self) -> bool {
        match self.ratio() {
            Some(percent) => percent > Self::NOTABLE_RISE_PERCENT,
            None => self.after > 0,
        }
    }

    fn differs(&self) -> bool {
        self.before != self.after
    }

    fn difference(&self) -> f64 {
        self.after as f64 - self.before as f64
    }

    fn percent(&self) -> String {
        match self.ratio() {
            Some(percent) => format!("{percent:+.2}%"),
            None if self.after == 0 => "0.00%".to_string(),
            None => "new".to_string(),
        }
    }

    fn ratio(&self) -> Option<f64> {
        (self.before > 0).then(|| self.difference() / self.before as f64 * Self::PERCENT)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    struct ComparisonFixture;

    impl ComparisonFixture {
        const PROGRAM_INSTRUCTIONS: u64 = 1_000_000;

        fn with_unmeasured(stages: &[(&str, u64)]) -> ResultSet {
            let orders = ProgramResult {
                program: "orders".to_string(),
                measurement: None,
            };
            ResultSet::new(vec![Self::inventory(stages), orders])
        }

        fn measured_only(stages: &[(&str, u64)]) -> ResultSet {
            ResultSet::new(vec![Self::inventory(stages)])
        }

        fn inventory(stages: &[(&str, u64)]) -> ProgramResult {
            let stages: BTreeMap<String, u64> = stages
                .iter()
                .map(|(stage, instructions)| (stage.to_string(), *instructions))
                .collect();
            ProgramResult {
                program: "inventory".to_string(),
                measurement: Some(Measurement::new(stages, Self::PROGRAM_INSTRUCTIONS)),
            }
        }

        fn expand(instructions: u64) -> (&'static str, u64) {
            ("expand", instructions)
        }

        fn source(instructions: u64) -> (&'static str, u64) {
            ("source", instructions)
        }

        fn report(before: &ResultSet, after: &ResultSet) -> String {
            Comparison::new(before, after).full_report()
        }

        fn brief(before: &ResultSet, after: &ResultSet) -> String {
            Comparison::new(before, after).brief_report()
        }
    }

    #[test]
    fn lists_each_stage_that_rose_notably() {
        let before = ComparisonFixture::with_unmeasured(&[ComparisonFixture::expand(500_000)]);
        let after = ComparisonFixture::with_unmeasured(&[ComparisonFixture::expand(510_000)]);
        let report = ComparisonFixture::report(&before, &after);
        assert!(report.starts_with(
            "These stages rose notably, or could not be compared:\n\n\
             - `inventory` `expand`: +2.00%\n\
             - `orders` was not measured before\n"
        ));
    }

    #[test]
    fn bolds_a_notable_rise() {
        let before = ComparisonFixture::with_unmeasured(&[ComparisonFixture::expand(500_000)]);
        let after = ComparisonFixture::with_unmeasured(&[ComparisonFixture::expand(510_000)]);
        let report = ComparisonFixture::report(&before, &after);
        assert!(report.contains("| inventory | expand | 500,000 | 510,000 | **+2.00%** |"));
        assert!(report.contains("| orders | not measured before |  |  |  |"));
    }

    #[test]
    fn reports_no_change_when_every_count_held() {
        let before = ComparisonFixture::measured_only(&[ComparisonFixture::expand(500_000)]);
        let after = ComparisonFixture::measured_only(&[ComparisonFixture::expand(500_000)]);
        assert_eq!(
            ComparisonFixture::brief(&before, &after),
            "No instruction count changed.\n"
        );
        assert!(
            ComparisonFixture::report(&before, &after)
                .starts_with("No stage of any program rose notably.\n")
        );
    }

    #[test]
    fn ranks_changes_by_their_share_of_the_program() {
        let before = ComparisonFixture::measured_only(&[
            ComparisonFixture::expand(500_000),
            ComparisonFixture::source(100),
        ]);
        let after = ComparisonFixture::measured_only(&[
            ComparisonFixture::expand(510_000),
            ComparisonFixture::source(150),
        ]);
        let report = ComparisonFixture::brief(&before, &after);
        assert!(report.starts_with(
            "Stages that rose notably, or could not be compared: 1.\n\n\
             The 3 largest of 3 changes:\n"
        ));
        let expand = report.find("| inventory | expand |").unwrap();
        let source = report.find("| inventory | source |").unwrap();
        assert!(expand < source);
        assert!(report.contains("| inventory | source | 100 | 150 | +50.00% |"));
    }

    #[test]
    fn counts_an_unmeasured_program_in_the_brief_report() {
        let results = ComparisonFixture::with_unmeasured(&[ComparisonFixture::expand(500_000)]);
        assert_eq!(
            ComparisonFixture::brief(&results, &results),
            "Stages that rose notably, or could not be compared: 1.\n\n"
        );
    }

    #[test]
    fn defines_a_notable_rise_in_the_full_report() {
        let results = ResultSet::new(vec![]);
        let report = ComparisonFixture::report(&results, &results);
        assert!(
            report.contains(
                "grows by more than 1% and by at least 0.1% of its program's instructions"
            )
        );
    }

    #[test]
    fn describes_a_change_from_zero() {
        assert_eq!(Change::new(0, 0).percent(), "0.00%");
        assert_eq!(Change::new(0, 5).percent(), "new");
        assert_eq!(Change::new(200, 100).percent(), "-50.00%");
    }
}
