mod code;
mod collection;
mod label;
mod render;

pub use code::DiagnosticCode;
pub use collection::Diagnostics;
pub use label::{Help, Label, Suggestion};
pub use render::DiagnosticRenderer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    severity: Severity,
    code: DiagnosticCode,
    title: String,
    primary: Label,
    secondary: Vec<Label>,
    notes: Vec<String>,
    helps: Vec<Help>,
}

impl Diagnostic {
    pub fn error(code: DiagnosticCode, title: impl Into<String>, primary: Label) -> Self {
        Self::new(Severity::Error, code, title.into(), primary)
    }

    pub fn warning(code: DiagnosticCode, title: impl Into<String>, primary: Label) -> Self {
        Self::new(Severity::Warning, code, title.into(), primary)
    }

    fn new(severity: Severity, code: DiagnosticCode, title: String, primary: Label) -> Self {
        Self {
            severity,
            code,
            title,
            primary,
            secondary: Vec::new(),
            notes: Vec::new(),
            helps: Vec::new(),
        }
    }

    pub fn with_label(mut self, label: Label) -> Self {
        self.secondary.push(label);
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.helps.push(Help::new(help.into()));
        self
    }

    pub fn with_suggestion(mut self, help: impl Into<String>, suggestion: Suggestion) -> Self {
        self.helps
            .push(Help::new(help.into()).with_suggestion(suggestion));
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    pub fn severity(&self) -> Severity {
        self.severity
    }

    pub fn code(&self) -> DiagnosticCode {
        self.code
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn labels(&self) -> impl Iterator<Item = &Label> {
        std::iter::once(&self.primary).chain(&self.secondary)
    }

    pub fn primary(&self) -> &Label {
        &self.primary
    }

    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    pub fn helps(&self) -> &[Help] {
        &self.helps
    }
}
