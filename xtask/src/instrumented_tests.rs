use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::ValueEnum;

use crate::cargo::Cargo;
use crate::failure::TaskFailure;
use crate::lcov::LcovReport;
use crate::test_shard::TestShard;

pub struct InstrumentedTests<'a> {
    root: &'a Path,
}

impl<'a> InstrumentedTests<'a> {
    const REPORT: &'static str = "target/coverage/lcov.info";
    const SHARD_REPORTS: &'static str = "target/coverage/shards";
    const UNMEASURED_PACKAGE: &'static str = "xtask";
    const FORGET_PREVIOUS_RUNS: [&'static str; 3] = ["llvm-cov", "clean", "--workspace"];
    const KEEP_BUILD_ARTIFACTS: &'static str = "--no-clean";

    pub fn new(root: &'a Path) -> Self {
        Self { root }
    }

    pub fn run(&self) -> Result<LcovReport, TaskFailure> {
        let report = self.root.join(Self::REPORT);
        self.measure(&report, &[])?;
        let text = Self::read(&report)?;
        Ok(LcovReport::parse(&text, self.root))
    }

    pub fn run_shard(&self, shard: TestShard) -> Result<(), TaskFailure> {
        self.measure(&self.shard_report(shard), &shard.cargo_arguments())
    }

    pub fn merge_shards(&self) -> Result<LcovReport, TaskFailure> {
        let texts = TestShard::value_variants()
            .iter()
            .map(|shard| Self::read(&self.shard_report(*shard)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(LcovReport::parse(&texts.concat(), self.root))
    }

    fn shard_report(&self, shard: TestShard) -> PathBuf {
        self.root
            .join(Self::SHARD_REPORTS)
            .join(shard.report_file())
    }

    fn read(report: &Path) -> Result<String, TaskFailure> {
        fs::read_to_string(report).map_err(|error| TaskFailure::unreadable(report, error))
    }

    fn measure(&self, report: &Path, selection: &[&str]) -> Result<(), TaskFailure> {
        Self::create_parent(report)?;
        self.cargo(|command| command.args(Self::FORGET_PREVIOUS_RUNS))?;
        self.cargo(|command| Self::instrumented_run(command, report, selection))
    }

    fn create_parent(report: &Path) -> Result<(), TaskFailure> {
        let Some(directory) = report.parent() else {
            return Ok(());
        };
        fs::create_dir_all(directory).map_err(|error| TaskFailure::unwritable(directory, error))
    }

    fn instrumented_run<'command>(
        command: &'command mut Command,
        report: &Path,
        selection: &[&str],
    ) -> &'command mut Command {
        command
            .args(["llvm-cov", Self::KEEP_BUILD_ARTIFACTS, "--workspace"])
            .args(["--exclude-from-report", Self::UNMEASURED_PACKAGE])
            .args(["--lcov", "--output-path"])
            .arg(report)
            .args(selection)
    }

    fn cargo(
        &self,
        arguments: impl FnOnce(&mut Command) -> &mut Command,
    ) -> Result<(), TaskFailure> {
        let mut command = Cargo::command();
        command.current_dir(self.root);
        let output = arguments(&mut command)
            .output()
            .map_err(|error| TaskFailure::tool_missing("cargo llvm-cov", error))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(TaskFailure::tests_failed(&output))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::InstrumentedTests;

    #[test]
    fn an_instrumented_run_ends_with_the_shard_selection() {
        let mut command = Command::new("cargo");
        let selection = ["--test", "stages"];
        InstrumentedTests::instrumented_run(&mut command, Path::new("report.info"), &selection);
        let arguments: Vec<_> = command.get_args().collect();
        assert_eq!(arguments[arguments.len() - selection.len()..], selection);
    }

    #[test]
    fn merging_refuses_a_missing_shard_report() {
        let tests = InstrumentedTests::new(Path::new("/nonexistent/workspace"));
        assert!(tests.merge_shards().is_err());
    }
}
