use std::sync::Arc;

use anstream::ColorChoice;

use super::capped_report::CappedReport;
use super::json_replies::{JsonObject, JsonReplies};
use super::standard_error::StandardError;
use crate::diagnostic::{Diagnostic, DiagnosticRenderer, Diagnostics};
use crate::source::{SourceMap, TextPosition};

pub trait DiagnosticOutput {
    fn write(&self, sources: &SourceMap, diagnostics: &Diagnostics);
    fn write_note(&self, sources: &SourceMap, message: &str);
}

pub struct RenderedDiagnostics {
    standard_error: ColorChoice,
}

pub struct JsonDiagnostics {
    replies: Arc<JsonReplies>,
}

struct JsonDiagnostic<'a> {
    sources: &'a SourceMap,
    renderer: DiagnosticRenderer<'a>,
}

impl DiagnosticOutput for RenderedDiagnostics {
    fn write(&self, sources: &SourceMap, diagnostics: &Diagnostics) {
        StandardError::write_line(CappedReport::new(self.renderer(sources)).render(diagnostics));
    }

    fn write_note(&self, sources: &SourceMap, message: &str) {
        StandardError::write_line(self.renderer(sources).render_note(message));
    }
}

impl RenderedDiagnostics {
    pub fn new(standard_error: ColorChoice) -> Self {
        Self { standard_error }
    }

    fn renderer<'a>(&self, sources: &'a SourceMap) -> DiagnosticRenderer<'a> {
        match self.standard_error {
            ColorChoice::Never => DiagnosticRenderer::plain(sources),
            _ => DiagnosticRenderer::styled(sources),
        }
    }
}

impl JsonDiagnostics {
    const FIELD: &'static str = "diagnostics";

    pub fn new(replies: Arc<JsonReplies>) -> Self {
        Self { replies }
    }
}

impl DiagnosticOutput for JsonDiagnostics {
    fn write(&self, sources: &SourceMap, diagnostics: &Diagnostics) {
        let json = JsonDiagnostic::new(sources);
        let entries: Vec<JsonObject> = diagnostics
            .in_source_order()
            .into_iter()
            .map(|diagnostic| json.of(diagnostic))
            .collect();
        let reply = JsonObject::listing(Self::FIELD, entries);
        let _ = self.replies.send(reply);
    }

    fn write_note(&self, sources: &SourceMap, message: &str) {
        StandardError::write_line(DiagnosticRenderer::plain(sources).render_note(message));
    }
}

impl<'a> JsonDiagnostic<'a> {
    fn new(sources: &'a SourceMap) -> Self {
        Self {
            sources,
            renderer: DiagnosticRenderer::plain(sources),
        }
    }

    fn of(&self, diagnostic: &Diagnostic) -> JsonObject {
        let severity = JsonObject::new("severity", diagnostic.severity_name());
        let coded = match diagnostic.code() {
            Some(code) => severity.with("code", code.as_str()),
            None => severity,
        };
        let placed = self.with_place(coded.with("message", diagnostic.title()), diagnostic);
        placed.with("rendered", self.renderer.render(diagnostic))
    }

    fn with_place(&self, object: JsonObject, diagnostic: &Diagnostic) -> JsonObject {
        let span = diagnostic.primary().span();
        let file = self.sources.file(span.source());
        let start = TextPosition::of_offset(file.text(), span.range().start);
        let end = TextPosition::of_offset(file.text(), span.range().end);
        object
            .with("file", file.name())
            .with("line", start.line())
            .with("column", start.column())
            .with("end_line", end.line())
            .with("end_column", end.column())
    }
}
