use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

use super::measurement::Measurement;
use super::table::{GroupedCount, MarkdownTable};
use crate::failure::TaskFailure;

#[derive(Debug, PartialEq, Eq)]
pub struct ProgramResult {
    pub program: String,
    pub measurement: Option<Measurement>,
}

pub struct ResultSet {
    programs: Vec<ProgramResult>,
}

impl ResultSet {
    const FIELD_SEPARATOR: char = '\t';
    const STAGE_SEPARATOR: char = '=';

    pub fn new(programs: Vec<ProgramResult>) -> Self {
        Self { programs }
    }

    pub fn load(path: &Path) -> Result<Self, TaskFailure> {
        let text =
            fs::read_to_string(path).map_err(|error| TaskFailure::unreadable(path, error))?;
        let programs = text
            .lines()
            .map(|line| {
                Self::parse_program(line).ok_or_else(|| TaskFailure::malformed_results(path, line))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { programs })
    }

    pub fn save(&self, path: &Path) -> Result<(), TaskFailure> {
        fs::write(path, self.serialize()).map_err(|error| TaskFailure::unwritable(path, error))
    }

    pub fn programs(&self) -> &[ProgramResult] {
        &self.programs
    }

    pub fn find(&self, program: &str) -> Option<&ProgramResult> {
        self.programs
            .iter()
            .find(|result| result.program == program)
    }

    fn serialize(&self) -> String {
        self.programs.iter().map(Self::serialize_program).collect()
    }

    fn serialize_program(result: &ProgramResult) -> String {
        let stages: String = result
            .measurement
            .iter()
            .flat_map(Measurement::rows)
            .filter(|(stage, _)| *stage != Measurement::OTHER)
            .map(|(stage, instructions)| {
                format!(
                    "{}{stage}{}{instructions}",
                    Self::FIELD_SEPARATOR,
                    Self::STAGE_SEPARATOR
                )
            })
            .collect();
        format!("{}{stages}\n", result.program)
    }

    fn parse_program(line: &str) -> Option<ProgramResult> {
        let mut fields = line.split(Self::FIELD_SEPARATOR);
        let program = fields.next()?.to_string();
        let mut stages = fields
            .map(Self::parse_stage)
            .collect::<Option<BTreeMap<_, _>>>()?;
        let measurement = match stages.remove(Measurement::TOTAL) {
            Some(total) => Some(Measurement::new(stages, total)),
            None if stages.is_empty() => None,
            None => return None,
        };
        Some(ProgramResult {
            program,
            measurement,
        })
    }

    fn parse_stage(field: &str) -> Option<(String, u64)> {
        let (stage, instructions) = field.split_once(Self::STAGE_SEPARATOR)?;
        Some((stage.to_string(), instructions.parse().ok()?))
    }
}

impl ResultSet {
    const REPORT_HEADER: [&'static str; 3] = ["Program", "Stage", "Instructions"];

    fn report_table(&self) -> MarkdownTable {
        let mut table = MarkdownTable::new(&Self::REPORT_HEADER);
        self.programs
            .iter()
            .flat_map(Self::report_rows)
            .for_each(|row| table.push(row));
        table
    }

    fn report_rows(result: &ProgramResult) -> Vec<Vec<String>> {
        let Some(measurement) = &result.measurement else {
            return vec![vec![
                result.program.clone(),
                "rejected".to_string(),
                String::new(),
            ]];
        };
        measurement
            .rows()
            .map(|(stage, instructions)| {
                vec![
                    result.program.clone(),
                    stage.to_string(),
                    GroupedCount::new(instructions).to_string(),
                ]
            })
            .collect()
    }
}

impl fmt::Display for ResultSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}\n{}",
            self.report_table(),
            Measurement::method()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ResultsFixture;

    impl ResultsFixture {
        fn results() -> ResultSet {
            let stages = BTreeMap::from([("expand".to_string(), 60)]);
            ResultSet::new(vec![
                ProgramResult {
                    program: "inventory".to_string(),
                    measurement: Some(Measurement::new(stages, 100)),
                },
                ProgramResult {
                    program: "unsupported".to_string(),
                    measurement: None,
                },
            ])
        }

        fn scratch_file(name: &str) -> std::path::PathBuf {
            std::env::temp_dir().join(format!("xtask-bench-{}-{name}", std::process::id()))
        }
    }

    #[test]
    fn reloads_saved_results() {
        let path = ResultsFixture::scratch_file("results.tsv");
        ResultsFixture::results().save(&path).unwrap();
        let reloaded = ResultSet::load(&path).unwrap();
        assert_eq!(reloaded.programs(), ResultsFixture::results().programs());
    }

    #[test]
    fn reports_a_rejected_program() {
        let report = ResultsFixture::results().to_string();
        assert!(report.contains("| inventory | other | 40 |"));
        assert!(report.contains("| unsupported | rejected |  |"));
    }

    #[test]
    fn rejects_a_stage_without_a_total() {
        let path = ResultsFixture::scratch_file("no_total.tsv");
        fs::write(&path, "inventory\texpand=1\n").unwrap();
        let failure = ResultSet::load(&path).err().unwrap();
        assert!(failure.to_string().contains("inventory"));
    }
}
