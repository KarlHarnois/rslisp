use std::io;
use std::path::PathBuf;

use super::exit_status::ExitStatus;
use super::standard_error::StandardError;
use super::temporary_directory::TemporaryDirectory;
use super::terminal::{BuildTarget, EvaluationRequest, ModuleOfSources, Terminal};
use super::test_entries::TestEntries;
use super::test_report::{TestOutcome, TestReport};
use super::test_run::{TestRun, TestRunError};
use super::value_marker::ValueMarker;
use crate::build::BuildProfile;
use crate::core::ModulePath;
use crate::emit::EmittedCrate;
use crate::expand::{ExpansionSession, Program};

pub struct TestCommand {
    entries: TestEntries,
    selection: TestSelection,
    untested: Option<ModulePath>,
    profile: BuildProfile,
}

pub enum TestSelection {
    Every,
    Containing(String),
    Exactly(String),
}

impl TestCommand {
    const RUNNER_SOURCE: &'static str = "<test runner>";
    const CRATE_NAME: &'static str = "tests";

    pub fn new(
        path: PathBuf,
        selection: TestSelection,
        untested: Option<String>,
        profile: BuildProfile,
    ) -> Self {
        Self {
            entries: TestEntries::new(path),
            selection,
            untested: untested.map(ModulePath::new),
            profile,
        }
    }

    pub fn run(self, terminal: &mut Terminal) -> ExitStatus {
        match self.test(terminal) {
            Ok(status) | Err(status) => status,
        }
    }

    fn test(&self, terminal: &mut Terminal) -> Result<ExitStatus, ExitStatus> {
        let module = self.entries.load(terminal)?;
        let marker = ValueMarker::random();
        let program = self.runner_program(terminal, module, &marker)?;
        let checked = terminal.check_expanded(&program)?;
        let emitted = terminal.emit_checked(&checked)?;
        let outcome = self.build_and_run(terminal, &emitted, &marker)?;
        self.note_no_selected_test(terminal, &outcome);
        Ok(Self::status(&outcome))
    }

    fn runner_program(
        &self,
        terminal: &mut Terminal,
        module: ModuleOfSources,
        marker: &ValueMarker,
    ) -> Result<Program, ExitStatus> {
        let request = EvaluationRequest {
            module,
            evaluated: Vec::new(),
        };
        let runner = self.runner_text(marker);
        let program = terminal
            .within_evaluation(&request, |session| self.entered_runner(session, &runner))?;
        program.ok_or_else(|| self.report_unloaded_module())
    }

    fn entered_runner(
        &self,
        mut session: ExpansionSession<'_>,
        runner: &str,
    ) -> Result<Option<Program>, crate::diagnostic::Diagnostics> {
        if let Some(module) = &self.untested
            && !session.loads(module)
        {
            return Ok(None);
        }
        let source = session.add_source(Self::RUNNER_SOURCE, runner);
        let entry = session.test_runner_entry(source)?;
        let mut program = session.finish()?;
        program.enter(&entry);
        Ok(Some(program))
    }

    fn runner_text(&self, marker: &ValueMarker) -> String {
        let untested = self
            .untested
            .as_ref()
            .map(|module| format!(" :untested {module}"))
            .unwrap_or_default();
        format!("(run-tests :marker \"{}\"{untested})", marker.text())
    }

    fn build_and_run(
        &self,
        terminal: &mut Terminal,
        emitted: &EmittedCrate,
        marker: &ValueMarker,
    ) -> Result<TestOutcome, ExitStatus> {
        let directory = TemporaryDirectory::create().map_err(Self::report_no_build_directory)?;
        let binary = directory.path().join(Self::CRATE_NAME);
        let target = BuildTarget {
            directory: directory.path().to_path_buf(),
            crate_name: Self::CRATE_NAME,
            output: &binary,
        };
        terminal.compile(emitted, &target, self.profile)?;
        let mut report = TestReport::new(io::stdout().lock());
        TestRun::new(&binary, self.selection.arguments())
            .run(marker.records(), &mut report)
            .map_err(Self::report_run_error)
    }

    fn note_no_selected_test(&self, terminal: &Terminal, outcome: &TestOutcome) {
        if outcome.selected > 0 {
            return;
        }
        if let Some(note) = self.selection.unmatched_note() {
            terminal.print_note(&note);
        }
    }

    fn status(outcome: &TestOutcome) -> ExitStatus {
        if outcome.all_passed {
            ExitStatus::Success
        } else {
            ExitStatus::ProgramErrors
        }
    }

    fn report_unloaded_module(&self) -> ExitStatus {
        let module = self
            .untested
            .as_ref()
            .expect("only `--untested` names a module");
        StandardError::write_line(format_args!(
            "error: the program does not load module `{module}`"
        ));
        ExitStatus::UsageError
    }

    fn report_no_build_directory(error: io::Error) -> ExitStatus {
        StandardError::write_line(format_args!(
            "error: cannot create a build directory: {error}"
        ));
        ExitStatus::Environment
    }

    fn report_run_error(error: TestRunError) -> ExitStatus {
        match error {
            TestRunError::NotStarted => ExitStatus::Environment,
            TestRunError::Unwritable(error) if error.kind() == io::ErrorKind::BrokenPipe => {
                ExitStatus::Success
            }
            TestRunError::Unwritable(error) => {
                StandardError::write_line(format_args!(
                    "error: cannot write standard output: {error}"
                ));
                ExitStatus::UsageError
            }
        }
    }
}

impl TestSelection {
    const EXACT_FLAG: &'static str = "--exact";

    pub fn of(filter: Option<String>, exact: Option<String>) -> Self {
        match (filter, exact) {
            (_, Some(name)) => Self::Exactly(name),
            (Some(filter), None) => Self::Containing(filter),
            (None, None) => Self::Every,
        }
    }

    fn arguments(&self) -> Vec<String> {
        match self {
            Self::Every => Vec::new(),
            Self::Containing(filter) => vec![filter.clone()],
            Self::Exactly(name) => vec![Self::EXACT_FLAG.to_string(), name.clone()],
        }
    }

    fn unmatched_note(&self) -> Option<String> {
        match self {
            Self::Every => None,
            Self::Containing(filter) => Some(format!("no test name contains `{filter}`")),
            Self::Exactly(name) => Some(format!("no test is named `{name}`")),
        }
    }
}
