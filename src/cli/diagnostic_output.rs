use std::io::IsTerminal;
use std::sync::Arc;

use super::capped_report::CappedReport;
use super::json_replies::{JsonObject, JsonReplies};
use crate::diagnostic::{Diagnostic, DiagnosticRenderer, Diagnostics};
use crate::source::{SourceMap, TextPosition};

pub trait DiagnosticOutput {
    fn write(&self, sources: &SourceMap, diagnostics: &Diagnostics);
}

pub struct RenderedDiagnostics;

pub struct JsonDiagnostics {
    replies: Arc<JsonReplies>,
}

struct JsonDiagnostic<'a> {
    sources: &'a SourceMap,
    renderer: DiagnosticRenderer<'a>,
}

impl DiagnosticOutput for RenderedDiagnostics {
    fn write(&self, sources: &SourceMap, diagnostics: &Diagnostics) {
        eprintln!(
            "{}",
            CappedReport::new(Self::renderer(sources)).render(diagnostics)
        );
    }
}

impl RenderedDiagnostics {
    fn renderer(sources: &SourceMap) -> DiagnosticRenderer<'_> {
        if std::io::stderr().is_terminal() {
            DiagnosticRenderer::styled(sources)
        } else {
            DiagnosticRenderer::plain(sources)
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
