mod code;
mod collection;
mod counted;
mod label;
mod render;
mod similar_name;

pub use code::DiagnosticCode;
pub use collection::Diagnostics;
pub use counted::Counted;
pub use label::{Help, Label, Suggestion};
pub use render::DiagnosticRenderer;
pub use similar_name::SimilarName;

#[derive(Clone, Debug)]
pub struct Diagnostic {
    report: Box<Report>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug)]
struct Report {
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
        Self::new(Severity::Error, code, title, primary)
    }

    pub fn warning(code: DiagnosticCode, title: impl Into<String>, primary: Label) -> Self {
        Self::new(Severity::Warning, code, title, primary)
    }

    fn new(
        severity: Severity,
        code: DiagnosticCode,
        title: impl Into<String>,
        primary: Label,
    ) -> Self {
        Self {
            report: Box::new(Report {
                severity,
                code,
                title: title.into(),
                primary,
                secondary: Vec::new(),
                notes: Vec::new(),
                helps: Vec::new(),
            }),
        }
    }

    pub fn is_error(&self) -> bool {
        self.report.severity == Severity::Error
    }

    pub fn with_label(mut self, label: Label) -> Self {
        self.report.secondary.push(label);
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.report.notes.push(note.into());
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.report.helps.push(Help::new(help.into()));
        self
    }

    pub fn with_suggestion(mut self, help: impl Into<String>, suggestion: Suggestion) -> Self {
        self.report
            .helps
            .push(Help::new(help.into()).with_suggestion(suggestion));
        self
    }

    pub fn code(&self) -> &DiagnosticCode {
        &self.report.code
    }

    pub fn title(&self) -> &str {
        &self.report.title
    }

    pub fn labels(&self) -> impl Iterator<Item = &Label> {
        std::iter::once(&self.report.primary).chain(&self.report.secondary)
    }

    pub fn primary(&self) -> &Label {
        &self.report.primary
    }

    pub fn notes(&self) -> &[String] {
        &self.report.notes
    }

    pub fn helps(&self) -> &[Help] {
        &self.report.helps
    }
}
