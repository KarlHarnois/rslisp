use std::fs;
use std::io::IsTerminal;
use std::path::Path;

use super::exit_status::ExitStatus;
use crate::diagnostic::{DiagnosticRenderer, Diagnostics};
use crate::source::{SourceId, SourceMap};

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

    pub fn sources(&self) -> &SourceMap {
        &self.sources
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

    fn report_unreadable(path: &Path, error: &std::io::Error) -> ExitStatus {
        eprintln!("error: cannot read `{}`: {error}", path.display());
        ExitStatus::UsageError
    }
}
