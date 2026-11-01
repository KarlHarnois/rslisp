use std::collections::{BTreeMap, BTreeSet};

use super::measurement::Measurement;
use crate::failure::TaskFailure;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryPointCheck {
    Enforced,
    Skipped,
}

pub struct ProfileReader<'a> {
    crate_names: &'a BTreeSet<String>,
    entry_point_check: EntryPointCheck,
}

impl<'a> ProfileReader<'a> {
    const ENTRY_MODULE: &'static str = "cli";
    const STAGE_ENTRY_POINTS: [&'static str; 4] = [
        "expand::Expander::expand",
        "check::ProgramChecker::check",
        "emit::RustEmitter::emit",
        "build::formatting::EmittedFormatting::apply",
    ];

    pub fn new(crate_names: &'a BTreeSet<String>, entry_point_check: EntryPointCheck) -> Self {
        Self {
            crate_names,
            entry_point_check,
        }
    }

    pub fn read(&self, profile: &str) -> Result<Measurement, TaskFailure> {
        let mut scan = ProfileScan::new(self);
        profile.lines().for_each(|line| scan.consume(line));
        scan.finish()
    }

    fn missing_entry_point(&self, entered: &BTreeSet<&str>) -> Option<Vec<String>> {
        if self.entry_point_check == EntryPointCheck::Skipped {
            return None;
        }
        Self::STAGE_ENTRY_POINTS
            .iter()
            .map(|entry_point| self.in_each_crate(entry_point))
            .find(|candidates| {
                !candidates
                    .iter()
                    .any(|candidate| entered.contains(candidate.as_str()))
            })
    }

    fn in_each_crate(&self, path: &str) -> Vec<String> {
        self.crate_names
            .iter()
            .map(|crate_name| format!("{crate_name}::{path}"))
            .collect()
    }

    fn stage_of_call(&self, caller: &str, callee: &str) -> Option<String> {
        let callee_module = self.module_of(callee)?;
        let leaves_entry = self.module_of(caller) == Some(Self::ENTRY_MODULE)
            && callee_module != Self::ENTRY_MODULE;
        leaves_entry.then(|| callee_module.to_string())
    }

    fn module_of<'name>(&self, function: &'name str) -> Option<&'name str> {
        let item = function.trim_start_matches('<');
        self.crate_names
            .iter()
            .find_map(|crate_name| Self::module_within(item, crate_name))
    }

    fn module_within<'name>(item: &'name str, crate_name: &str) -> Option<&'name str> {
        let path = item.strip_prefix(crate_name)?.strip_prefix("::")?;
        let length = path
            .find(|character: char| !character.is_alphanumeric() && character != '_')
            .unwrap_or(path.len());
        (length > 0).then(|| &path[..length])
    }
}

enum ProfileLine<'a> {
    Events(&'a str),
    Summary(&'a str),
    Caller(&'a str),
    Callee(&'a str),
    Call,
    Other,
}

impl<'a> ProfileLine<'a> {
    fn classify(line: &'a str) -> Self {
        if let Some(header) = line.strip_prefix("events:") {
            Self::Events(header)
        } else if let Some(counts) = line.strip_prefix("summary:") {
            Self::Summary(counts)
        } else if let Some(name) = line.strip_prefix("fn=") {
            Self::Caller(name)
        } else if let Some(name) = line.strip_prefix("cfn=") {
            Self::Callee(name)
        } else if line.starts_with("calls=") {
            Self::Call
        } else {
            Self::Other
        }
    }
}

struct ProfileScan<'reader, 'text> {
    reader: &'reader ProfileReader<'reader>,
    instruction_column: Option<usize>,
    summary: Option<Vec<u64>>,
    caller: &'text str,
    callee: &'text str,
    call_cost_follows: bool,
    stages: BTreeMap<String, u64>,
    entered: BTreeSet<&'text str>,
}

impl<'reader, 'text> ProfileScan<'reader, 'text> {
    const INSTRUCTIONS_EVENT: &'static str = "Ir";

    fn new(reader: &'reader ProfileReader<'reader>) -> Self {
        Self {
            reader,
            instruction_column: None,
            summary: None,
            caller: "",
            callee: "",
            call_cost_follows: false,
            stages: BTreeMap::new(),
            entered: BTreeSet::new(),
        }
    }

    fn consume(&mut self, line: &'text str) {
        if self.call_cost_follows {
            self.call_cost_follows = false;
            self.record_call(line);
            return;
        }
        match ProfileLine::classify(line) {
            ProfileLine::Events(header) => self.instruction_column = Self::column_of(header),
            ProfileLine::Summary(counts) => self.summary = Some(Self::counts(counts)),
            ProfileLine::Caller(name) => self.caller = name,
            ProfileLine::Callee(name) => self.callee = name,
            ProfileLine::Call => self.call_cost_follows = true,
            ProfileLine::Other => {}
        }
    }

    fn record_call(&mut self, cost_line: &str) {
        let (Some(stage), Some(column)) = (
            self.reader.stage_of_call(self.caller, self.callee),
            self.instruction_column,
        ) else {
            return;
        };
        let counts = Self::counts(cost_line.split_once(' ').map_or("", |(_, counts)| counts));
        *self.stages.entry(stage).or_default() += Self::count_at(&counts, column);
        self.entered.insert(self.callee);
    }

    fn finish(self) -> Result<Measurement, TaskFailure> {
        let column = self
            .instruction_column
            .ok_or(TaskFailure::malformed_profile("events"))?;
        let summary = self
            .summary
            .ok_or(TaskFailure::malformed_profile("summary"))?;
        if let Some(candidates) = self.reader.missing_entry_point(&self.entered) {
            return Err(TaskFailure::stage_entry_missing(&candidates));
        }
        Ok(Measurement::new(
            self.stages,
            Self::count_at(&summary, column),
        ))
    }

    fn column_of(header: &str) -> Option<usize> {
        header
            .split_whitespace()
            .position(|event| event == Self::INSTRUCTIONS_EVENT)
    }

    fn count_at(counts: &[u64], column: usize) -> u64 {
        counts.get(column).copied().unwrap_or(0)
    }

    fn counts(fields: &str) -> Vec<u64> {
        fields
            .split_whitespace()
            .map(|field| field.parse().unwrap_or(0))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ProfileFixture;

    impl ProfileFixture {
        const ONE_CRATE: &'static str = include_str!("../../fixtures/bench/callgrind.out");
        const TWO_CRATES: &'static str =
            include_str!("../../fixtures/bench/callgrind-two-crates.out");
        const COMPILER: [&'static str; 1] = ["compiler"];
        const COMPILER_AND_FRONT_END: [&'static str; 2] = ["compiler", "compiler_front"];
        const STAGE_ROWS: [(&'static str, u64); 6] = [
            ("build", 20),
            ("check", 300),
            ("emit", 50),
            ("expand", 500),
            ("other", 130),
            ("total", 1000),
        ];

        fn read(
            profile: &str,
            crate_names: &[&str],
            entry_point_check: EntryPointCheck,
        ) -> Result<Measurement, TaskFailure> {
            let crate_names = crate_names.iter().map(ToString::to_string).collect();
            ProfileReader::new(&crate_names, entry_point_check).read(profile)
        }

        fn rows(measurement: &Measurement) -> Vec<(String, u64)> {
            measurement
                .rows()
                .map(|(stage, instructions)| (stage.to_string(), instructions))
                .collect()
        }

        fn stage_rows() -> Vec<(String, u64)> {
            Self::STAGE_ROWS
                .iter()
                .map(|(stage, instructions)| (stage.to_string(), *instructions))
                .collect()
        }

        fn without_emit_entry(profile: &str) -> String {
            profile.replace("RustEmitter::emit\n", "RustEmitter::lower\n")
        }
    }

    #[test]
    fn charges_calls_out_of_the_entry_module_to_the_callee_module() {
        let measurement = ProfileFixture::read(
            ProfileFixture::ONE_CRATE,
            &ProfileFixture::COMPILER,
            EntryPointCheck::Enforced,
        )
        .unwrap();
        assert_eq!(
            ProfileFixture::rows(&measurement),
            ProfileFixture::stage_rows()
        );
    }

    #[test]
    fn charges_calls_into_another_workspace_crate_to_the_callee_module() {
        let measurement = ProfileFixture::read(
            ProfileFixture::TWO_CRATES,
            &ProfileFixture::COMPILER_AND_FRONT_END,
            EntryPointCheck::Enforced,
        )
        .unwrap();
        assert_eq!(
            ProfileFixture::rows(&measurement),
            ProfileFixture::stage_rows()
        );
    }

    #[test]
    fn rejects_a_profile_that_never_enters_a_stage() {
        let failure = ProfileFixture::read(
            &ProfileFixture::without_emit_entry(ProfileFixture::ONE_CRATE),
            &ProfileFixture::COMPILER,
            EntryPointCheck::Enforced,
        )
        .unwrap_err();
        assert!(
            failure
                .to_string()
                .contains("into `compiler::emit::RustEmitter::emit`,")
        );
    }

    #[test]
    fn names_the_missing_entry_point_in_each_workspace_crate() {
        let failure = ProfileFixture::read(
            &ProfileFixture::without_emit_entry(ProfileFixture::TWO_CRATES),
            &ProfileFixture::COMPILER_AND_FRONT_END,
            EntryPointCheck::Enforced,
        )
        .unwrap_err();
        assert!(failure.to_string().contains(
            "into `compiler::emit::RustEmitter::emit` or `compiler_front::emit::RustEmitter::emit`,"
        ));
    }

    #[test]
    fn skips_the_entry_points_of_a_baseline() {
        let measurement = ProfileFixture::read(
            &ProfileFixture::without_emit_entry(ProfileFixture::ONE_CRATE),
            &ProfileFixture::COMPILER,
            EntryPointCheck::Skipped,
        );
        assert!(measurement.is_ok());
    }

    #[test]
    fn rejects_a_profile_without_a_summary() {
        let failure = ProfileFixture::read(
            "events: Ir\n",
            &ProfileFixture::COMPILER,
            EntryPointCheck::Enforced,
        );
        assert!(failure.unwrap_err().to_string().contains("summary"));
    }
}
