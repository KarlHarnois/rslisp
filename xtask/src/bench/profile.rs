use std::collections::BTreeMap;

use super::measurement::Measurement;
use crate::failure::TaskFailure;

pub struct ProfileReader<'a> {
    crate_name: &'a str,
}

impl<'a> ProfileReader<'a> {
    const ENTRY_MODULE: &'static str = "cli";

    pub fn new(crate_name: &'a str) -> Self {
        Self { crate_name }
    }

    pub fn read(&self, profile: &str) -> Result<Measurement, TaskFailure> {
        let mut scan = ProfileScan::new(self);
        profile.lines().for_each(|line| scan.consume(line));
        scan.finish()
    }

    fn stage_of_call(&self, caller: &str, callee: &str) -> Option<String> {
        let callee_module = self.module_of(callee)?;
        let leaves_entry = self.module_of(caller) == Some(Self::ENTRY_MODULE)
            && callee_module != Self::ENTRY_MODULE;
        leaves_entry.then(|| callee_module.to_string())
    }

    fn module_of<'name>(&self, function: &'name str) -> Option<&'name str> {
        let path = function
            .trim_start_matches('<')
            .strip_prefix(self.crate_name)?
            .strip_prefix("::")?;
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
    }

    fn finish(self) -> Result<Measurement, TaskFailure> {
        let column = self
            .instruction_column
            .ok_or(TaskFailure::malformed_profile("events"))?;
        let summary = self
            .summary
            .ok_or(TaskFailure::malformed_profile("summary"))?;
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

    const PROFILE: &str = include_str!("../../fixtures/bench/callgrind.out");

    #[test]
    fn charges_calls_out_of_the_entry_module_to_the_callee_module() {
        let measurement = ProfileReader::new("compiler").read(PROFILE).unwrap();
        let rows: Vec<(String, u64)> = measurement
            .rows()
            .map(|(stage, instructions)| (stage.to_string(), instructions))
            .collect();
        let expected = [
            ("check", 300),
            ("expand", 500),
            ("other", 200),
            ("total", 1000),
        ];
        let expected: Vec<(String, u64)> = expected
            .iter()
            .map(|(stage, instructions)| (stage.to_string(), *instructions))
            .collect();
        assert_eq!(rows, expected);
    }

    #[test]
    fn rejects_a_profile_without_a_summary() {
        let failure = ProfileReader::new("compiler").read("events: Ir\n");
        assert!(failure.unwrap_err().to_string().contains("summary"));
    }
}
