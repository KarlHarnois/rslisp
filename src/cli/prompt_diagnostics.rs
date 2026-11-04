use std::cell::RefCell;
use std::rc::Rc;

use super::diagnostic_output::DiagnosticOutput;
use super::error_location::{InputLocator, Location};
use super::name_records::{NameRecords, Tense};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::expand::{BoundName, ExpandProblems};
use crate::source::SourceMap;

#[derive(Default)]
pub struct PromptAnnotations {
    committed: NameRecords,
    pending: NameRecords,
    expression: Option<InputLocator>,
}

pub struct PromptDiagnostics {
    inner: Rc<dyn DiagnosticOutput>,
    annotations: Rc<RefCell<PromptAnnotations>>,
}

impl PromptDiagnostics {
    pub fn new(
        inner: Rc<dyn DiagnosticOutput>,
        annotations: Rc<RefCell<PromptAnnotations>>,
    ) -> Self {
        Self { inner, annotations }
    }
}

impl DiagnosticOutput for PromptDiagnostics {
    fn write(&self, sources: &SourceMap, diagnostics: &Diagnostics) {
        let annotations = self.annotations.borrow();
        let annotated = diagnostics
            .clone()
            .map(|diagnostic| annotations.annotated(diagnostic, sources));
        self.inner.write(sources, &annotated);
    }

    fn write_note(&self, sources: &SourceMap, message: &str) {
        self.inner.write_note(sources, message);
    }
}

impl PromptAnnotations {
    pub fn commit<'a>(
        &mut self,
        records: NameRecords,
        bound: impl IntoIterator<Item = &'a BoundName>,
    ) {
        self.committed.absorb(records);
        self.committed.forget(bound);
    }

    pub fn hold_pending(&mut self, pending: NameRecords) {
        self.pending = pending;
    }

    pub fn drop_pending(&mut self) {
        self.pending = NameRecords::default();
    }

    pub fn locate_expression(&mut self, locator: InputLocator) {
        self.expression = Some(locator);
    }

    pub fn forget_expression(&mut self) {
        self.expression = None;
    }

    fn annotated(&self, diagnostic: Diagnostic, sources: &SourceMap) -> Diagnostic {
        let noted = self.with_record_note(diagnostic);
        self.with_expression_help(noted, sources)
    }

    fn with_record_note(&self, diagnostic: Diagnostic) -> Diagnostic {
        let Some(missing) = diagnostic.missing_name() else {
            return diagnostic;
        };
        let note = self
            .pending
            .note(missing, Tense::Pending)
            .or_else(|| self.committed.note(missing, Tense::Done));
        match note {
            Some(note) => diagnostic.with_note(note),
            None => diagnostic,
        }
    }

    fn with_expression_help(&self, diagnostic: Diagnostic, sources: &SourceMap) -> Diagnostic {
        match &self.expression {
            Some(locator) if Self::lies_in_the_buffer(locator, &diagnostic, sources) => {
                ExpandProblems::checks_no_longer(diagnostic)
            }
            _ => diagnostic,
        }
    }

    fn lies_in_the_buffer(
        locator: &InputLocator,
        diagnostic: &Diagnostic,
        sources: &SourceMap,
    ) -> bool {
        let file_of = |source| sources.file(source).name().to_string();
        locator.locate(diagnostic, file_of) != Location::Entered
    }
}
