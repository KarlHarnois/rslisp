use std::env;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Map, Value};

use super::callgrind::{Callgrind, RunOutcome};
use super::results::Counters;
use crate::failure::TaskFailure;

pub struct CounterRun {
    executable: PathBuf,
    language_name: String,
}

impl CounterRun {
    const COUNTERS_FLAG: &'static str = "--counters";

    pub fn new(executable: PathBuf, language_name: String) -> Self {
        Self {
            executable,
            language_name,
        }
    }

    pub fn count_all(&self, programs: &[PathBuf]) -> Result<Vec<Counters>, TaskFailure> {
        programs.iter().map(|program| self.count(program)).collect()
    }

    fn count(&self, program: &Path) -> Result<Counters, TaskFailure> {
        let output = self.run(program)?;
        match RunOutcome::of(output.status) {
            RunOutcome::Accepted | RunOutcome::Rejected => {
                Self::read_report(program, &output.stderr)
            }
            RunOutcome::Crashed => Err(TaskFailure::run_crashed(
                program,
                output.status,
                &output.stderr,
            )),
        }
    }

    fn run(&self, program: &Path) -> Result<Output, TaskFailure> {
        self.command(program)
            .output()
            .map_err(|error| TaskFailure::tool_missing(&self.executable.to_string_lossy(), error))
    }

    fn command(&self, program: &Path) -> Command {
        let mut command = Command::new(&self.executable);
        command
            .arg0(&self.language_name)
            .env_clear()
            .env(
                Callgrind::SEARCH_PATH,
                env::var_os(Callgrind::SEARCH_PATH).unwrap_or_default(),
            )
            .arg(Self::COUNTERS_FLAG)
            .arg(Callgrind::FRONT_END_COMMAND)
            .arg(program.file_name().unwrap_or_default())
            .current_dir(program.parent().unwrap_or(Path::new(".")))
            .stdout(Stdio::null());
        command
    }

    fn read_report(program: &Path, stderr: &[u8]) -> Result<Counters, TaskFailure> {
        String::from_utf8_lossy(stderr)
            .lines()
            .last()
            .and_then(Self::parse_report)
            .ok_or_else(|| TaskFailure::counters_malformed(program, stderr))
    }

    fn parse_report(line: &str) -> Option<Counters> {
        let report: Map<String, Value> = serde_json::from_str(line).ok()?;
        report
            .into_iter()
            .map(|(counter, value)| Some((counter, value.as_u64()?)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    struct CounterRunFixture;

    impl CounterRunFixture {
        const PROGRAM: &'static str = "/checkout/corpus/inventory.rslisp";

        fn command() -> Command {
            let run = CounterRun::new(
                PathBuf::from("/checkout/target/release/compiler"),
                "ledger-lang".to_string(),
            );
            run.command(Path::new(Self::PROGRAM))
        }

        fn read(stderr: &str) -> Result<Counters, TaskFailure> {
            CounterRun::read_report(Path::new(Self::PROGRAM), stderr.as_bytes())
        }
    }

    #[test]
    fn passes_only_the_search_path() {
        let command = CounterRunFixture::command();
        let variables: Vec<&OsStr> = command.get_envs().map(|(variable, _)| variable).collect();
        assert_eq!(variables, [OsStr::new("PATH")]);
    }

    #[test]
    fn runs_from_the_directory_of_the_program() {
        let command = CounterRunFixture::command();
        assert_eq!(
            command.get_current_dir(),
            Some(Path::new("/checkout/corpus"))
        );
    }

    #[test]
    fn emits_the_bare_file_name_with_the_counters_flag() {
        let command = CounterRunFixture::command();
        let arguments: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(arguments, ["--counters", "emit", "inventory.rslisp"]);
    }

    #[test]
    fn reads_the_counters_from_the_last_line_of_standard_error() {
        let stderr = "warning: unused binding\n{\"allocator.allocated_bytes\":4096,\"emit.emitted_expressions\":3}\n";
        let counters = CounterRunFixture::read(stderr).unwrap();
        assert_eq!(
            counters,
            Counters::from([
                ("allocator.allocated_bytes".to_string(), 4096),
                ("emit.emitted_expressions".to_string(), 3),
            ])
        );
    }

    #[test]
    fn fails_on_a_last_line_that_is_not_json() {
        let failure =
            CounterRunFixture::read("{\"allocator.allocations\":1}\npanicked\n").unwrap_err();
        assert!(failure.to_string().contains("inventory.rslisp"));
    }

    #[test]
    fn fails_on_a_count_that_is_not_a_number() {
        let failure = CounterRunFixture::read("{\"allocator.allocations\":\"many\"}\n");
        assert!(failure.is_err());
    }

    #[test]
    fn fails_on_an_empty_standard_error() {
        assert!(CounterRunFixture::read("").is_err());
    }
}
