use std::env::{self, VarError};
use std::fmt::Display;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use anstream::ColorChoice;

use super::build_directory::BuildDirectory;
use super::diagnostic_output::{DiagnosticOutput, RenderedDiagnostics};
use super::exit_status::ExitStatus;
use super::stage_work::StageWork;
use crate::build::{BuildError, BuildProfile, EmittedFormatting, RustCompiler};
use crate::core::{Expr, ModulePath};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::emit::{EmittedCrate, RustEmitter};
use crate::expand::{Expander, ExpansionSession, PreludeState, Program};
use crate::modules::ModuleSources;
use crate::reader::{Form, Reader};
use crate::source::Span;
use crate::source::{SourceId, SourceMap, TextPosition};
use crate::types::TypedProgram;

pub struct BuildOptions {
    pub profile: BuildProfile,
    pub output: Option<PathBuf>,
}

pub struct EvaluationRequest {
    pub module: PathBuf,
    pub root: PathBuf,
    pub entry: SourceId,
    pub evaluated: Vec<Span>,
}

pub struct BuildTarget<'a> {
    pub directory: PathBuf,
    pub crate_name: &'a str,
    pub output: &'a Path,
}

pub struct Terminal {
    sources: SourceMap,
    work: StageWork,
    diagnostics: Rc<dyn DiagnosticOutput>,
}

impl Terminal {
    const BUILD_DIRECTORY: &str = "target/rslisp";
    const WORKING_DIRECTORY: &str = ".";
    const SAVED_PRELUDE: &[u8] = include_bytes!(env!("PRELUDE_STATE"));

    pub fn new(standard_error: ColorChoice) -> Self {
        Self::reporting_to(Rc::new(RenderedDiagnostics::new(standard_error)))
    }

    pub fn scratch(&self) -> Self {
        Self::reporting_to(self.diagnostic_output())
    }

    pub fn reporting_to(diagnostics: Rc<dyn DiagnosticOutput>) -> Self {
        Self {
            sources: SourceMap::default(),
            work: StageWork::default(),
            diagnostics,
        }
    }

    pub fn report_diagnostics_to(&mut self, diagnostics: Rc<dyn DiagnosticOutput>) {
        self.diagnostics = diagnostics;
    }

    pub fn diagnostic_output(&self) -> Rc<dyn DiagnosticOutput> {
        Rc::clone(&self.diagnostics)
    }

    pub fn work(&self) -> &StageWork {
        &self.work
    }

    pub fn load(&mut self, path: &Path) -> Result<SourceId, ExitStatus> {
        match ModuleSources::read_file(path) {
            Ok(text) => Ok(self.add_source(path, text)),
            Err(error) => Err(Self::report_unreadable(path, &error)),
        }
    }

    pub fn add_empty_source(&mut self, path: &Path) -> SourceId {
        self.add_source(path, String::new())
    }

    pub fn add_source(&mut self, path: &Path, text: String) -> SourceId {
        self.sources.add(path.display().to_string(), text)
    }

    pub fn text_of(&self, source: SourceId) -> String {
        self.sources.file(source).text().to_string()
    }

    pub fn add_named_source(&mut self, name: &str, text: &str) -> SourceId {
        self.sources.add(name, text)
    }

    pub fn add_source_inside(
        &mut self,
        enclosing: SourceId,
        position: TextPosition,
        text: &str,
    ) -> Option<SourceId> {
        self.sources.add_inside(enclosing, position, text)
    }

    pub fn read_source(&mut self, source: SourceId) -> Result<Vec<Form>, ExitStatus> {
        Reader::new(self.sources.file(source))
            .read()
            .map_err(|diagnostics| self.report(&diagnostics))
    }

    pub fn write_output(&self, output: impl Display) -> ExitStatus {
        match Self::write_standard_output(output) {
            Err(error) if error.kind() != ErrorKind::BrokenPipe => Self::report_unwritable(&error),
            _ => ExitStatus::Success,
        }
    }

    fn write_standard_output(output: impl Display) -> io::Result<()> {
        let mut standard_output = io::stdout().lock();
        write!(standard_output, "{output}")?;
        standard_output.flush()
    }

    pub fn build(&mut self, path: &Path, options: &BuildOptions) -> Result<PathBuf, ExitStatus> {
        let emitted = self.emit(path)?;
        let crate_name = Self::crate_name(path);
        let directory = Self::build_directory(path, &crate_name)?;
        let output = options
            .output
            .clone()
            .unwrap_or_else(|| directory.join(&crate_name));
        let target = BuildTarget {
            directory,
            crate_name: &crate_name,
            output: &output,
        };
        self.compile(&emitted, &target, options.profile)?;
        Ok(output)
    }

    pub fn compile(
        &self,
        emitted: &EmittedCrate,
        target: &BuildTarget<'_>,
        profile: BuildProfile,
    ) -> Result<(), ExitStatus> {
        RustCompiler::new(target.directory.clone(), profile)
            .compile(
                emitted,
                target.crate_name,
                target.output,
                Self::entry_span(),
            )
            .map_err(|error| self.report_build(error))
    }

    pub fn emit(&mut self, path: &Path) -> Result<EmittedCrate, ExitStatus> {
        let formatting = Self::formatting()?;
        let program = self.check(path)?;
        self.emit_formatted(&program, &formatting)
    }

    pub fn emit_checked(&mut self, program: &TypedProgram) -> Result<EmittedCrate, ExitStatus> {
        let formatting = Self::formatting()?;
        self.emit_formatted(program, &formatting)
    }

    fn emit_formatted(
        &mut self,
        program: &TypedProgram,
        formatting: &EmittedFormatting,
    ) -> Result<EmittedCrate, ExitStatus> {
        let emitted = RustEmitter::emit(program, Self::entry_span())
            .map_err(|diagnostics| self.report(&diagnostics))?;
        self.work.emitted_expressions = Some(emitted.emission.emitted_expressions);
        let written = match formatting.apply(&emitted) {
            Ok(formatted) => formatted,
            Err(error) => {
                eprintln!("note: {error}; writing unformatted Rust");
                emitted
            }
        };
        self.work.scanned_tokens = written
            .formatting
            .map(|formatting| formatting.scanned_tokens);
        Ok(written)
    }

    fn formatting() -> Result<EmittedFormatting, ExitStatus> {
        Self::rustfmt_time_limit().map(EmittedFormatting::within)
    }

    fn rustfmt_time_limit() -> Result<Duration, ExitStatus> {
        let variable = EmittedFormatting::time_limit_variable();
        match env::var(&variable) {
            Err(VarError::NotPresent) => Ok(EmittedFormatting::TIME_LIMIT),
            value => value
                .ok()
                .and_then(|milliseconds| milliseconds.parse().ok())
                .map(Duration::from_millis)
                .ok_or_else(|| Self::report_malformed_variable(&variable)),
        }
    }

    pub fn check(&mut self, path: &Path) -> Result<TypedProgram, ExitStatus> {
        let program = self.expand(path)?;
        self.check_expanded(&program)
    }

    pub fn check_expanded(&mut self, program: &Program) -> Result<TypedProgram, ExitStatus> {
        self.check_showing(program, |_| true)
    }

    pub fn check_showing(
        &mut self,
        program: &Program,
        shown_warning: impl FnMut(&Diagnostic) -> bool,
    ) -> Result<TypedProgram, ExitStatus> {
        let mut typed = crate::check::ProgramChecker::check(program)
            .map_err(|diagnostics| self.report(&diagnostics))?;
        self.work.inferred_expressions = Some(typed.checking.inferred_expressions);
        typed.warnings.retain(shown_warning);
        self.print_diagnostics(&typed.warnings);
        Ok(typed)
    }

    pub fn expand(&mut self, path: &Path) -> Result<Program, ExitStatus> {
        let entry = self.load(path)?;
        let program = self
            .expander(path)
            .expand(Self::module_path(path), entry)
            .map_err(|diagnostics| self.report(&diagnostics))?;
        self.work.lowered_forms = Some(program.expansion.lowered_forms);
        Ok(program)
    }

    pub fn read_text(&mut self, name: &str, text: &str) -> Result<Vec<Form>, ExitStatus> {
        let source = self.sources.add(name, text);
        self.read_source(source)
    }

    pub fn expand_expression(
        &mut self,
        path: &Path,
        entry: SourceId,
        (name, text): (&str, &str),
        expression: impl FnOnce(SourceId) -> Form,
    ) -> Result<Expr, ExitStatus> {
        self.within_session(path, entry, |mut session| {
            let source = session.add_source(name, text);
            session.expression(expression(source))
        })
    }

    pub fn within_session<T>(
        &mut self,
        path: &Path,
        entry: SourceId,
        work: impl FnOnce(ExpansionSession<'_>) -> Result<T, Diagnostics>,
    ) -> Result<T, ExitStatus> {
        self.expander(path)
            .open(Self::module_path(path), entry)
            .and_then(work)
            .map_err(|diagnostics| self.report(&diagnostics))
    }

    pub fn within_evaluation<T>(
        &mut self,
        request: &EvaluationRequest,
        work: impl FnOnce(ExpansionSession<'_>) -> Result<T, Diagnostics>,
    ) -> Result<T, ExitStatus> {
        self.expander_rooted(&request.root)
            .evaluating(request.evaluated.iter().copied())
            .open(Self::module_path(&request.module), request.entry)
            .and_then(work)
            .map_err(|diagnostics| self.report(&diagnostics))
    }

    fn expander(&mut self, path: &Path) -> Expander<'_> {
        self.expander_rooted(&Self::directory_of(path))
    }

    fn expander_rooted(&mut self, root: &Path) -> Expander<'_> {
        let directory = root.to_path_buf();
        let prelude =
            PreludeState::load(Self::SAVED_PRELUDE).expect("the embedded prelude state loads");
        Expander::with_prelude(
            &mut self.sources,
            ModuleSources::new(directory.clone(), directory),
            prelude,
        )
    }

    fn module_path(path: &Path) -> ModulePath {
        ModulePath::new(Self::module_name(path))
    }

    pub fn print_diagnostics(&self, diagnostics: &Diagnostics) {
        if diagnostics.is_empty() {
            return;
        }
        self.diagnostics.write(&self.sources, diagnostics);
    }

    pub fn report(&self, diagnostics: &Diagnostics) -> ExitStatus {
        if !diagnostics.has_errors() {
            return ExitStatus::Success;
        }
        self.diagnostics.write(&self.sources, diagnostics);
        ExitStatus::ProgramErrors
    }

    fn report_build(&self, error: BuildError) -> ExitStatus {
        match error {
            BuildError::Program(diagnostics) => self.report(&diagnostics),
            BuildError::Environment(message) => {
                eprintln!("error: {message}");
                ExitStatus::Environment
            }
        }
    }

    fn build_directory(path: &Path, crate_name: &str) -> Result<PathBuf, ExitStatus> {
        let working_directory = Self::canonical(Path::new(Self::WORKING_DIRECTORY))?;
        let entry_directory = Self::canonical(&Self::directory_of(path))?;
        Ok(
            BuildDirectory::new(PathBuf::from(Self::BUILD_DIRECTORY), working_directory)
                .for_entry(&entry_directory, crate_name),
        )
    }

    fn canonical(directory: &Path) -> Result<PathBuf, ExitStatus> {
        Path::new(Self::WORKING_DIRECTORY)
            .join(directory)
            .canonicalize()
            .map_err(|error| Self::report_unresolvable(directory, &error))
    }

    fn entry_span() -> Span {
        Span::new(SourceId::new(0), 0..0)
    }

    pub fn crate_name(path: &Path) -> String {
        Self::module_name(path).replace(|character: char| !character.is_ascii_alphanumeric(), "_")
    }

    pub fn directory_of(path: &Path) -> std::path::PathBuf {
        path.parent().map(Path::to_path_buf).unwrap_or_default()
    }

    fn module_name(path: &Path) -> String {
        path.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn report_unresolvable(directory: &Path, error: &std::io::Error) -> ExitStatus {
        eprintln!("error: cannot resolve `{}`: {error}", directory.display());
        ExitStatus::UsageError
    }

    fn report_malformed_variable(variable: &str) -> ExitStatus {
        eprintln!("error: `{variable}` must be a whole number of milliseconds");
        ExitStatus::UsageError
    }

    fn report_unwritable(error: &io::Error) -> ExitStatus {
        eprintln!("error: cannot write standard output: {error}");
        ExitStatus::UsageError
    }

    fn report_unreadable(path: &Path, error: &std::io::Error) -> ExitStatus {
        eprintln!("error: cannot read `{}`: {error}", path.display());
        ExitStatus::UsageError
    }
}
