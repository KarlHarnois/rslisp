use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

use super::header::ResultHeader;
use super::measurement::Measurement;
use super::table::{GroupedCount, MarkdownTable};
use crate::failure::TaskFailure;

pub type Counters = BTreeMap<String, u64>;

#[derive(Debug, PartialEq, Eq)]
pub struct ProgramResult {
    pub program: String,
    pub measurement: Option<Measurement>,
    pub counters: Counters,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MalformedLine {
    pub line: String,
}

impl MalformedLine {
    fn new(line: &str) -> Self {
        Self {
            line: line.to_string(),
        }
    }
}

pub struct ResultSet {
    header: ResultHeader,
    programs: Vec<ProgramResult>,
}

impl ResultSet {
    const FIELD_SEPARATOR: char = '\t';
    const VALUE_SEPARATOR: char = '=';
    const COUNTER_GROUP_SEPARATOR: char = '.';

    pub fn new(programs: Vec<ProgramResult>) -> Self {
        Self {
            header: ResultHeader::default(),
            programs,
        }
    }

    pub fn with_header(self, header: ResultHeader) -> Self {
        Self { header, ..self }
    }

    pub fn load(path: &Path) -> Result<Self, TaskFailure> {
        let text =
            fs::read_to_string(path).map_err(|error| TaskFailure::unreadable(path, error))?;
        Self::parse(&text)
            .map_err(|malformed| TaskFailure::malformed_results(path, &malformed.line))
    }

    pub fn parse(text: &str) -> Result<Self, MalformedLine> {
        let header = ResultHeader::read(text);
        let programs = text
            .lines()
            .skip(header.line_count())
            .map(|line| Self::parse_program(line).ok_or_else(|| MalformedLine::new(line)))
            .collect::<Result<_, _>>()?;
        Ok(Self { header, programs })
    }

    pub fn save(&self, path: &Path) -> Result<(), TaskFailure> {
        fs::write(path, self.serialize()).map_err(|error| TaskFailure::unwritable(path, error))
    }

    pub fn method_key(&self) -> Option<&str> {
        self.header.method_key()
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
        let programs: String = self.programs.iter().map(Self::serialize_program).collect();
        format!("{}{programs}", self.header.serialize())
    }

    fn serialize_program(result: &ProgramResult) -> String {
        let stages = result
            .measurement
            .iter()
            .flat_map(Measurement::rows)
            .filter(|(stage, _)| *stage != Measurement::OTHER);
        let counters = result
            .counters
            .iter()
            .map(|(counter, value)| (counter.as_str(), *value));
        let fields: String = stages.chain(counters).map(Self::serialize_field).collect();
        format!("{}{fields}\n", result.program)
    }

    fn serialize_field((name, value): (&str, u64)) -> String {
        format!(
            "{}{name}{}{value}",
            Self::FIELD_SEPARATOR,
            Self::VALUE_SEPARATOR
        )
    }

    fn parse_program(line: &str) -> Option<ProgramResult> {
        let mut fields = line.split(Self::FIELD_SEPARATOR);
        let program = fields.next()?.to_string();
        let (counter_fields, stage_fields): (Vec<&str>, Vec<&str>) =
            fields.partition(|field| Self::is_counter(field));
        Some(ProgramResult {
            program,
            measurement: Self::parse_measurement(&stage_fields)?,
            counters: Self::parse_fields(&counter_fields)?,
        })
    }

    fn is_counter(field: &str) -> bool {
        field
            .split_once(Self::VALUE_SEPARATOR)
            .is_some_and(|(name, _)| name.contains(Self::COUNTER_GROUP_SEPARATOR))
    }

    fn parse_measurement(stage_fields: &[&str]) -> Option<Option<Measurement>> {
        let mut stages = Self::parse_fields(stage_fields)?;
        match stages.remove(Measurement::TOTAL) {
            Some(total) => Some(Some(Measurement::new(stages, total))),
            None if stages.is_empty() => Some(None),
            None => None,
        }
    }

    fn parse_fields(fields: &[&str]) -> Option<BTreeMap<String, u64>> {
        fields
            .iter()
            .map(|field| Self::parse_field(field))
            .collect()
    }

    fn parse_field(field: &str) -> Option<(String, u64)> {
        let (name, value) = field.split_once(Self::VALUE_SEPARATOR)?;
        Some((name.to_string(), value.parse().ok()?))
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
                    counters: Counters::from([("allocator.allocated_bytes".to_string(), 4096)]),
                },
                ProgramResult {
                    program: "unsupported".to_string(),
                    measurement: None,
                    counters: Counters::new(),
                },
            ])
        }

        fn header() -> ResultHeader {
            ResultHeader::new(
                "rustc 1.0.0 (fake 2020-01-01)".to_string(),
                "valgrind-0.0.0".to_string(),
            )
            .keyed("a1-b2-c3-d4".to_string())
        }

        fn fixture(name: &str) -> std::path::PathBuf {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/bench")
                .join(name)
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
    fn reloads_the_header_of_saved_results() {
        let path = ResultsFixture::scratch_file("headed.tsv");
        let results = ResultsFixture::results().with_header(ResultsFixture::header());
        results.save(&path).unwrap();
        let reloaded = ResultSet::load(&path).unwrap();
        assert_eq!(reloaded.header, ResultsFixture::header());
        assert_eq!(reloaded.programs(), results.programs());
    }

    #[test]
    fn loads_a_file_without_a_header() {
        let results = ResultSet::load(&ResultsFixture::fixture("headless-results.tsv")).unwrap();
        assert_eq!(results.method_key(), None);
        assert_eq!(results.programs().len(), 2);
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

    #[test]
    fn loads_the_counters_after_the_stages() {
        let results = ResultSet::load(&ResultsFixture::fixture("counted-results.tsv")).unwrap();
        let inventory = results.find("inventory").unwrap();
        assert_eq!(
            inventory.measurement.as_ref().map(Measurement::total),
            Some(100)
        );
        assert_eq!(
            inventory.counters,
            Counters::from([
                ("allocator.allocated_bytes".to_string(), 4096),
                ("allocator.peak_live_bytes".to_string(), 1024),
            ])
        );
    }

    #[test]
    fn loads_the_counters_of_a_rejected_program() {
        let results = ResultSet::load(&ResultsFixture::fixture("counted-results.tsv")).unwrap();
        let unsupported = results.find("unsupported").unwrap();
        assert_eq!(unsupported.measurement, None);
        assert_eq!(unsupported.counters.get("allocator.allocations"), Some(&7));
    }

    #[test]
    fn loads_a_line_without_counters() {
        let results = ResultSet::parse("warehouse\texpand=1\tcheck=2\ttotal=5\n").unwrap();
        let warehouse = results.find("warehouse").unwrap();
        assert_eq!(
            warehouse.measurement.as_ref().map(Measurement::total),
            Some(5)
        );
        assert!(warehouse.counters.is_empty());
    }

    #[test]
    fn rejects_a_malformed_counter_value() {
        let malformed = "inventory\ttotal=100\tallocator.allocated_bytes=many";
        let failure = ResultSet::parse(&format!("{malformed}\n")).err().unwrap();
        assert_eq!(failure.line, malformed);
    }
}
