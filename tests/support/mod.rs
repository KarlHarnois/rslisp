mod formatting;
mod headers;
mod reports;
mod saved_prelude;

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Output, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use knave::build::{BuildError, BuildProfile, EmittedFormatting, RustCompiler};
use knave::core::{BindingArena, CorePrinter, ExprKind, Item, ItemKind, Leaf, Literal, ModulePath};
use knave::diagnostic::{DiagnosticRenderer, Diagnostics};
use knave::emit::{EmittedCrate, RustEmitter};
use knave::expand::{Expander, PreludeState};
use knave::format::{FormatFailure, SourceFormatter};
use knave::language::Language;
use knave::modules::ModuleSources;
use knave::reader::Reader;
use knave::source::{SourceId, SourceMap, Span};
use knave::types::TypedProgram;
use rustc_hash::FxHashMap;

pub use formatting::{FormatFailures, ShapeLookup};
pub use headers::HeaderAgreement;
pub use reports::FixtureReports;
pub use saved_prelude::SavedPreludeAgreement;

pub struct Fixture {
    sources: SourceMap,
    entry: SourceId,
    path: PathBuf,
    prelude: Option<PreludeState>,
}

impl Fixture {
    const FIXTURE_ROOT: &str = "tests/fixtures";
    const RUN_DEADLINE: Duration = Duration::from_secs(10);
    const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(10);
    const PANIC_THREAD: &str = "thread '";
    const PANIC_LOCATION: &str = " panicked at ";
    const PATH_SEPARATOR: char = '/';

    pub fn load(path: &Path) -> Self {
        let text = fs::read_to_string(path).expect("fixture is readable");
        let mut sources = SourceMap::default();
        let entry = sources.add(Self::display_name(path), text);
        Self {
            sources,
            entry,
            path: path.to_path_buf(),
            prelude: None,
        }
    }

    pub fn with_saved_prelude(self, prelude: PreludeState) -> Self {
        Self {
            prelude: Some(prelude),
            ..self
        }
    }

    pub fn stage_report(self) -> String {
        match self.stage_name().as_str() {
            "expand" => self.expand_report(),
            "check" => self.check_report(),
            _ => self.emit_report(),
        }
    }

    pub fn read_report(&self) -> String {
        match Reader::new(self.sources.file(self.entry)).read() {
            Ok(forms) => forms.iter().map(|form| format!("{form}\n")).collect(),
            Err(diagnostics) => self.render(&diagnostics),
        }
    }

    pub fn format_report(&self) -> String {
        let input = self.sources.file(self.entry);
        let lookup = ShapeLookup::beside(&self.path);
        match SourceFormatter::default().format(input, &lookup) {
            Ok(formatted) => format!("{}\n---\n{formatted}", input.text()),
            Err(FormatFailure::Unreadable(diagnostics)) => self.render(&diagnostics),
            Err(failure) => panic!(
                "formatting {} fails its check: {failure:?}",
                self.path.display()
            ),
        }
    }

    pub fn expand_report(mut self) -> String {
        match self.expand() {
            Ok(program) => CorePrinter::new(&program.bindings).module(program.entry_module()),
            Err(diagnostics) => self.render(&diagnostics),
        }
    }

    pub fn string_definitions(mut self) -> FxHashMap<String, String> {
        let program = self.expand().unwrap_or_else(|diagnostics| {
            panic!("the fixture expands:\n{}", self.render(&diagnostics))
        });
        program
            .entry_module()
            .items
            .iter()
            .filter_map(|item| Self::string_definition(&program.bindings, item))
            .collect()
    }

    fn string_definition(bindings: &BindingArena, item: &Item) -> Option<(String, String)> {
        let ItemKind::Def(binding, value) = &item.kind else {
            return None;
        };
        let ExprKind::Leaf(Leaf::Literal(Literal::String(text))) = value.kind() else {
            return None;
        };
        Some((bindings.get(*binding).name().to_string(), text.to_string()))
    }

    pub fn check_report(mut self) -> String {
        match self.checked() {
            Ok(program) if program.warnings.is_empty() => program.entry_signatures(),
            Ok(program) => format!(
                "{}\n\n{}",
                self.render(&program.warnings),
                program.entry_signatures()
            ),
            Err(diagnostics) => self.render(&diagnostics),
        }
    }

    pub fn emit_report(mut self) -> String {
        match self.emit() {
            Ok(emitted) => emitted.code,
            Err(diagnostics) => self.render(&diagnostics),
        }
    }

    pub fn run_report(mut self) -> String {
        let emitted = match self.emit() {
            Ok(emitted) => emitted,
            Err(diagnostics) => return self.render(&diagnostics),
        };
        let directory = self.build_directory();
        let binary = directory.join(self.module_name());
        match RustCompiler::new(directory, BuildProfile::Debug).compile(
            &emitted,
            "fixture",
            &binary,
            self.entry_span(),
        ) {
            Ok(()) => self.report_of(&binary),
            Err(BuildError::Program(diagnostics)) => self.render(&diagnostics),
            Err(BuildError::Environment(message)) => {
                panic!("the build environment failed: {message}")
            }
            Err(BuildError::Interrupted) => panic!("rustc was interrupted"),
        }
    }

    fn build_directory(&self) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{}-run-{}-{}-{}",
            Language::NAME,
            std::process::id(),
            self.stage_name(),
            self.module_name()
        ))
    }

    fn stage_name(&self) -> String {
        let stage = self.directory();
        let name = stage
            .file_name()
            .expect("fixtures live in a stage directory");
        name.to_string_lossy().into_owned()
    }

    fn arguments(&self) -> Vec<String> {
        let arguments = self.path.with_extension("arguments");
        match fs::read_to_string(arguments) {
            Ok(text) => text.lines().map(str::to_string).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn emit(&mut self) -> Result<EmittedCrate, Diagnostics> {
        let program = self.checked()?;
        let emitted = RustEmitter::emit(&program, self.entry_span())?;
        Ok(EmittedFormatting::within(EmittedFormatting::TIME_LIMIT)
            .apply(&emitted)
            .expect("rustfmt formats the emitted Rust"))
    }

    fn checked(&mut self) -> Result<TypedProgram, Diagnostics> {
        let program = self.expand()?;
        knave::check::ProgramChecker::check(&program)
    }

    fn entry_span(&self) -> Span {
        Span::new(self.entry, 0..0)
    }

    fn report_of(&self, binary: &Path) -> String {
        let output = self.run(binary);
        let stdout = String::from_utf8(output.stdout).expect("fixtures print text");
        let stderr = String::from_utf8(output.stderr).expect("fixtures print text");
        if stderr.is_empty() && output.status.success() {
            stdout
        } else {
            let code = output
                .status
                .code()
                .map_or_else(|| "a signal".to_string(), |code| code.to_string());
            let stderr = Self::without_run_details(&stderr);
            format!("{stdout}stderr:\n{stderr}exit code: {code}\n")
        }
    }

    fn without_run_details(stderr: &str) -> String {
        stderr
            .split_inclusive('\n')
            .map(|line| Self::panic_header(line).unwrap_or_else(|| line.to_string()))
            .collect()
    }

    fn panic_header(line: &str) -> Option<String> {
        let rest = line.strip_prefix(Self::PANIC_THREAD)?;
        let (thread, location) = rest.split_once(Self::PANIC_LOCATION)?;
        let name = thread.split(Self::PANIC_THREAD.chars().last()?).next()?;
        let file = location.rsplit(Self::PATH_SEPARATOR).next()?;
        Some(format!(
            "{}{name}'{}{file}",
            Self::PANIC_THREAD,
            Self::PANIC_LOCATION
        ))
    }

    fn run(&self, binary: &Path) -> Output {
        let mut child = Command::new(binary)
            .args(self.arguments())
            .current_dir(self.directory())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the compiled fixture runs");
        let input = child.stdin.take().expect("the fixture's stdin is piped");
        let offer = Self::offer(input, self.standard_input());
        let stdout = Self::collect(child.stdout.take().expect("the fixture's stdout is piped"));
        let stderr = Self::collect(child.stderr.take().expect("the fixture's stderr is piped"));
        let status = Self::await_exit(&mut child);
        offer.join().expect("the standard input offer ends");
        Output {
            status,
            stdout: stdout.join().expect("the fixture's stdout is collected"),
            stderr: stderr.join().expect("the fixture's stderr is collected"),
        }
    }

    fn collect(mut stream: impl Read + Send + 'static) -> JoinHandle<Vec<u8>> {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            stream
                .read_to_end(&mut bytes)
                .expect("the fixture's output is readable");
            bytes
        })
    }

    fn await_exit(child: &mut Child) -> ExitStatus {
        let deadline = Instant::now() + Self::RUN_DEADLINE;
        loop {
            if let Some(status) = child
                .try_wait()
                .expect("the compiled fixture's status is readable")
            {
                return status;
            }
            if Instant::now() >= deadline {
                Self::stop_past_deadline(child);
            }
            thread::sleep(Self::EXIT_POLL_INTERVAL);
        }
    }

    fn stop_past_deadline(child: &mut Child) -> ! {
        child.kill().expect("the compiled fixture stops");
        panic!("the compiled fixture ran past its deadline");
    }

    fn standard_input(&self) -> Vec<u8> {
        fs::read(self.path.with_extension("stdin")).unwrap_or_default()
    }

    fn offer(mut input: ChildStdin, bytes: Vec<u8>) -> JoinHandle<()> {
        thread::spawn(move || {
            let _ignored_by_a_program_that_stops_reading = input.write_all(&bytes);
        })
    }

    fn expand(&mut self) -> Result<knave::expand::Program, Diagnostics> {
        let module_sources = ModuleSources::new(self.directory(), self.display_directory());
        let module_path = ModulePath::new(self.module_name());
        let expander = match self.prelude.take() {
            Some(prelude) => Expander::with_prelude(&mut self.sources, module_sources, prelude),
            None => Expander::new(&mut self.sources, module_sources),
        };
        expander.expand(module_path, self.entry)
    }

    fn render(&self, diagnostics: &Diagnostics) -> String {
        DiagnosticRenderer::plain(&self.sources).render_all(diagnostics)
    }

    fn directory(&self) -> PathBuf {
        self.path
            .parent()
            .expect("fixtures live in a directory")
            .to_path_buf()
    }

    fn display_directory(&self) -> PathBuf {
        PathBuf::from(Self::display_name(&self.directory()))
    }

    fn module_name(&self) -> String {
        let stem = self.path.file_stem().expect("fixtures have a file name");
        stem.to_string_lossy().into_owned()
    }

    fn display_name(path: &Path) -> String {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(Self::FIXTURE_ROOT);
        path.strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string()
    }
}

#[cfg(test)]
mod run_details_tests {
    use super::Fixture;

    #[test]
    fn stripping_run_details_keeps_every_line_ending() {
        let stderr = "first\r\nthread 'main' (42) panicked at /tmp/run/main.rs:3:9:\nlast";
        assert_eq!(
            Fixture::without_run_details(stderr),
            "first\r\nthread 'main' panicked at main.rs:3:9:\nlast"
        );
    }
}
