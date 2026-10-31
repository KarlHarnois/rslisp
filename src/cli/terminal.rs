use std::env::{self, VarError};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::build_directory::BuildDirectory;
use super::capped_report::CappedReport;
use super::exit_status::ExitStatus;
use crate::build::{BuildError, BuildProfile, EmittedFormatting, RustCompiler};
use crate::core::{Expr, ModulePath};
use crate::diagnostic::{DiagnosticRenderer, Diagnostics};
use crate::emit::{EmittedCrate, RustEmitter};
use crate::expand::{Expander, Program};
use crate::modules::ModuleSources;
use crate::reader::{Form, Reader};
use crate::source::Span;
use crate::source::{SourceId, SourceMap};
use crate::types::TypedProgram;

pub struct BuildOptions {
    pub profile: BuildProfile,
    pub output: Option<PathBuf>,
}

pub struct Terminal {
    sources: SourceMap,
}

impl Terminal {
    const BUILD_DIRECTORY: &str = "target/rslisp";
    const WORKING_DIRECTORY: &str = ".";

    pub fn new() -> Self {
        Self {
            sources: SourceMap::default(),
        }
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

    fn add_source(&mut self, path: &Path, text: String) -> SourceId {
        self.sources.add(path.display().to_string(), text)
    }

    pub fn build(&mut self, path: &Path, options: &BuildOptions) -> Result<PathBuf, ExitStatus> {
        let emitted = self.emit(path)?;
        let crate_name = Self::crate_name(path);
        let directory = Self::build_directory(path, &crate_name)?;
        let output = options
            .output
            .clone()
            .unwrap_or_else(|| directory.join(&crate_name));
        RustCompiler::new(directory, options.profile)
            .compile(&emitted, &crate_name, &output, Self::entry_span())
            .map(|()| output)
            .map_err(|error| self.report_build(error))
    }

    pub fn emit(&mut self, path: &Path) -> Result<EmittedCrate, ExitStatus> {
        let formatting = Self::formatting()?;
        let program = self.check(path)?;
        let emitted = RustEmitter::emit(&program, Self::entry_span())
            .map_err(|diagnostics| self.report(&diagnostics))?;
        match formatting.apply(&emitted) {
            Ok(formatted) => Ok(formatted),
            Err(error) => {
                eprintln!("note: {error}; writing unformatted Rust");
                Ok(emitted)
            }
        }
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
        let typed = crate::check::ProgramChecker::check(&program)
            .map_err(|diagnostics| self.report(&diagnostics))?;
        self.print_warnings(&typed.warnings);
        Ok(typed)
    }

    pub fn expand(&mut self, path: &Path) -> Result<Program, ExitStatus> {
        let entry = self.load(path)?;
        self.expander(path)
            .expand(Self::module_path(path), entry)
            .map_err(|diagnostics| self.report(&diagnostics))
    }

    pub fn read_text(&mut self, name: &str, text: &str) -> Result<Vec<Form>, ExitStatus> {
        let source = self.sources.add(name, text);
        Reader::new(self.sources.file(source))
            .read()
            .map_err(|diagnostics| self.report(&diagnostics))
    }

    pub fn expand_expression(
        &mut self,
        path: &Path,
        entry: SourceId,
        expression: Form,
    ) -> Result<Expr, ExitStatus> {
        self.expander(path)
            .open(Self::module_path(path), entry)
            .and_then(|mut session| session.expression(expression))
            .map_err(|diagnostics| self.report(&diagnostics))
    }

    fn expander(&mut self, path: &Path) -> Expander<'_> {
        let directory = Self::directory_of(path);
        Expander::new(
            &mut self.sources,
            ModuleSources::new(directory.clone(), directory),
        )
    }

    fn module_path(path: &Path) -> ModulePath {
        ModulePath::new(Self::module_name(path))
    }

    fn print_warnings(&self, warnings: &Diagnostics) {
        if warnings.is_empty() {
            return;
        }
        eprintln!("{}", self.capped_report().render(warnings));
    }

    pub fn report(&self, diagnostics: &Diagnostics) -> ExitStatus {
        if !diagnostics.has_errors() {
            return ExitStatus::Success;
        }
        eprintln!("{}", self.capped_report().render(diagnostics));
        ExitStatus::ProgramErrors
    }

    fn capped_report(&self) -> CappedReport<'_> {
        CappedReport::new(self.renderer())
    }

    fn renderer(&self) -> DiagnosticRenderer<'_> {
        if std::io::stderr().is_terminal() {
            DiagnosticRenderer::styled(&self.sources)
        } else {
            DiagnosticRenderer::plain(&self.sources)
        }
    }

    fn report_build(&self, error: BuildError) -> ExitStatus {
        match error {
            BuildError::Program(diagnostics) => self.report(&diagnostics),
            BuildError::Environment(message) => {
                eprintln!("error: {message}");
                ExitStatus::UsageError
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

    fn crate_name(path: &Path) -> String {
        Self::module_name(path).replace(|character: char| !character.is_ascii_alphanumeric(), "_")
    }

    fn directory_of(path: &Path) -> std::path::PathBuf {
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

    fn report_unreadable(path: &Path, error: &std::io::Error) -> ExitStatus {
        eprintln!("error: cannot read `{}`: {error}", path.display());
        ExitStatus::UsageError
    }
}
