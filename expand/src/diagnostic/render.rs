use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;

use annotate_snippets::{AnnotationKind, Group, Level, Patch, Renderer, Snippet};

use super::source_window::{LineIndex, SourceWindow};
use super::{Diagnostic, Diagnostics, Help, Label, Suggestion};
use crate::source::{SourceId, SourceMap};

pub struct DiagnosticRenderer<'a> {
    sources: &'a SourceMap,
    renderer: Renderer,
    line_indexes: RefCell<HashMap<SourceId, LineIndex>>,
}

impl<'a> DiagnosticRenderer<'a> {
    pub fn plain(sources: &'a SourceMap) -> Self {
        Self {
            sources,
            renderer: Renderer::plain(),
            line_indexes: RefCell::default(),
        }
    }

    pub fn styled(sources: &'a SourceMap) -> Self {
        Self {
            sources,
            renderer: Renderer::styled(),
            line_indexes: RefCell::default(),
        }
    }

    pub fn render_all(&self, diagnostics: &Diagnostics) -> String {
        let rendered: Vec<String> = diagnostics
            .in_source_order()
            .into_iter()
            .map(|diagnostic| self.render(diagnostic))
            .collect();
        rendered.join("\n\n")
    }

    pub fn render(&self, diagnostic: &Diagnostic) -> String {
        let mut report = vec![self.main_group(diagnostic)];
        report.extend(
            diagnostic
                .helps()
                .iter()
                .filter_map(|help| self.suggestion_group(help)),
        );
        self.renderer.render(&report)
    }

    fn main_group<'d>(&'d self, diagnostic: &'d Diagnostic) -> Group<'d> {
        let level = if diagnostic.is_error() {
            Level::ERROR
        } else {
            Level::WARNING
        };
        let title = level
            .primary_title(diagnostic.title())
            .id(diagnostic.code().as_str());
        let snippets = self
            .label_sources(diagnostic)
            .into_iter()
            .map(|source| self.labeled_snippet(diagnostic, source));
        let notes = diagnostic
            .notes()
            .iter()
            .map(|note| Level::NOTE.message(note.as_str()));
        let plain_helps = diagnostic
            .helps()
            .iter()
            .filter(|help| help.suggestion().is_none())
            .map(|help| Level::HELP.message(help.message()));
        title
            .elements(snippets)
            .elements(notes)
            .elements(plain_helps)
    }

    fn label_sources(&self, diagnostic: &Diagnostic) -> Vec<SourceId> {
        let mut sources: Vec<SourceId> = Vec::new();
        for label in diagnostic.labels() {
            if !sources.contains(&label.span().source()) {
                sources.push(label.span().source());
            }
        }
        sources
    }

    fn labeled_snippet<'d>(
        &'d self,
        diagnostic: &'d Diagnostic,
        source: SourceId,
    ) -> Snippet<'d, annotate_snippets::Annotation<'d>> {
        let primary_span = diagnostic.primary().span();
        let labels_in_source = || {
            diagnostic
                .labels()
                .filter(move |label| label.span().source() == source)
        };
        let window = self.window(source, labels_in_source().map(|label| label.span().range()));
        let annotations = labels_in_source()
            .map(|label| Self::annotation(label, label.span() == primary_span, &window));
        self.snippet(source, &window).annotations(annotations)
    }

    fn annotation<'d>(
        label: &'d Label,
        is_primary: bool,
        window: &SourceWindow,
    ) -> annotate_snippets::Annotation<'d> {
        let kind = if is_primary {
            AnnotationKind::Primary
        } else {
            AnnotationKind::Context
        };
        kind.span(window.relative(label.span().range()))
            .label(label.message())
    }

    fn suggestion_group<'d>(&'d self, help: &'d Help) -> Option<Group<'d>> {
        let suggestion = help.suggestion()?;
        Some(
            Group::with_title(Level::HELP.secondary_title(help.message()))
                .element(self.patched_snippet(suggestion)),
        )
    }

    fn patched_snippet<'d>(&'d self, suggestion: &'d Suggestion) -> Snippet<'d, Patch<'d>> {
        let span = suggestion.span();
        let window = self.window(span.source(), std::iter::once(span.range()));
        self.snippet(span.source(), &window).patch(Patch::new(
            window.relative(span.range()),
            suggestion.replacement(),
        ))
    }

    fn snippet<'d, T: Clone>(&'d self, source: SourceId, window: &SourceWindow) -> Snippet<'d, T> {
        let file = self.sources.file(source);
        Snippet::source(window.text(file.text()))
            .line_start(window.first_line())
            .path(file.name())
    }

    fn window(&self, source: SourceId, ranges: impl Iterator<Item = Range<usize>>) -> SourceWindow {
        let text = self.sources.file(source).text();
        self.line_indexes
            .borrow_mut()
            .entry(source)
            .or_insert_with(|| LineIndex::new(text))
            .window_covering(ranges)
    }
}

#[cfg(test)]
mod tests {
    use super::DiagnosticRenderer;
    use crate::diagnostic::{Diagnostic, DiagnosticCode, Diagnostics, Label};
    use crate::source::{SourceMap, Span};

    const ESCAPE: char = '\u{1b}';

    #[test]
    fn styled_rendering_colors_the_report() {
        let mut sources = SourceMap::default();
        let source = sources.add("sample.rslisp", "(def total)");
        let mut diagnostics = Diagnostics::default();
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::MALFORMED_FORM,
            "malformed `def`",
            Label::new(Span::new(source, 0..11), "this does not fit"),
        ));
        let styled = DiagnosticRenderer::styled(&sources).render_all(&diagnostics);
        let plain = DiagnosticRenderer::plain(&sources).render_all(&diagnostics);
        assert!(styled.contains(ESCAPE));
        assert!(!plain.contains(ESCAPE));
    }
}
