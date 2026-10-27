use std::fs;
use std::path::Path;
use std::process::Command;

use crate::cargo::Cargo;
use crate::failure::TaskFailure;
use crate::lcov::LcovReport;

pub struct InstrumentedTests<'a> {
    root: &'a Path,
}

impl<'a> InstrumentedTests<'a> {
    const REPORT: &'static str = "target/coverage/lcov.info";
    const UNMEASURED_PACKAGE: &'static str = "xtask";
    const FORGET_PREVIOUS_RUNS: [&'static str; 3] = ["llvm-cov", "clean", "--workspace"];
    const KEEP_BUILD_ARTIFACTS: &'static str = "--no-clean";

    pub fn new(root: &'a Path) -> Self {
        Self { root }
    }

    pub fn run(&self) -> Result<LcovReport, TaskFailure> {
        let report = self.root.join(Self::REPORT);
        Self::create_parent(&report)?;
        self.cargo(|command| command.args(Self::FORGET_PREVIOUS_RUNS))?;
        self.cargo(|command| Self::measure(command, &report))?;
        let text =
            fs::read_to_string(&report).map_err(|error| TaskFailure::unreadable(&report, error))?;
        Ok(LcovReport::parse(&text, self.root))
    }

    fn create_parent(report: &Path) -> Result<(), TaskFailure> {
        let Some(directory) = report.parent() else {
            return Ok(());
        };
        fs::create_dir_all(directory).map_err(|error| TaskFailure::unwritable(directory, error))
    }

    fn measure<'command>(command: &'command mut Command, report: &Path) -> &'command mut Command {
        command
            .args(["llvm-cov", Self::KEEP_BUILD_ARTIFACTS, "--workspace"])
            .args(["--exclude-from-report", Self::UNMEASURED_PACKAGE])
            .args(["--lcov", "--output-path"])
            .arg(report)
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
