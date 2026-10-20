use std::fs;
use std::io::IsTerminal;
use std::path::Path;

use super::exit_status::ExitStatus;
use crate::core::ModulePath;
use crate::diagnostic::{DiagnosticRenderer, Diagnostics};
use crate::expand::{Expander, Program};
use crate::modules::ModuleSources;
use crate::source::{SourceId, SourceMap};
use crate::types::{TypeChecker, TypedProgram};

pub struct Terminal {
    sources: SourceMap,
}

impl Terminal {
    pub fn new() -> Self {
        Self {
            sources: SourceMap::default(),
        }
    }

    pub fn load(&mut self, path: &Path) -> Result<SourceId, ExitStatus> {
        match fs::read_to_string(path) {
            Ok(text) => Ok(self.sources.add(path.display().to_string(), text)),
            Err(error) => Err(Self::report_unreadable(path, &error)),
        }
    }

    pub fn check(&mut self, path: &Path) -> Result<TypedProgram, ExitStatus> {
        let program = self.expand(path)?;
        TypeChecker::check(program).map_err(|diagnostics| self.report(&diagnostics))
    }

    pub fn expand(&mut self, path: &Path) -> Result<Program, ExitStatus> {
        let entry = self.load(path)?;
        let directory = Self::directory_of(path);
        let module_sources = ModuleSources::new(directory.clone(), directory);
        let module_path = ModulePath::new(Self::module_name(path));
        Expander::new(&mut self.sources, module_sources)
            .expand(module_path, entry)
            .map_err(|diagnostics| self.report(&diagnostics))
    }

    pub fn report(&self, diagnostics: &Diagnostics) -> ExitStatus {
        if !diagnostics.is_empty() {
            eprintln!("{}", self.renderer().render_all(diagnostics));
        }
        if diagnostics.has_errors() {
            ExitStatus::ProgramErrors
        } else {
            ExitStatus::Success
        }
    }

    fn renderer(&self) -> DiagnosticRenderer<'_> {
        if std::io::stderr().is_terminal() {
            DiagnosticRenderer::styled(&self.sources)
        } else {
            DiagnosticRenderer::plain(&self.sources)
        }
    }

    fn directory_of(path: &Path) -> std::path::PathBuf {
        path.parent().map(Path::to_path_buf).unwrap_or_default()
    }

    fn module_name(path: &Path) -> String {
        path.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn report_unreadable(path: &Path, error: &std::io::Error) -> ExitStatus {
        eprintln!("error: cannot read `{}`: {error}", path.display());
        ExitStatus::UsageError
    }
}
