use std::cell::RefCell;
use std::ops::Range;

use annotate_snippets::renderer::{DEFAULT_LINE_NUM_STYLE, Style};
use annotate_snippets::{AnnotationKind, Group, Level, Patch, Renderer, Snippet};
use rustc_hash::FxHashMap;

use super::source_window::{LineIndex, SourceWindow};
use super::{Diagnostic, Diagnostics, Help, Label, Suggestion};
use crate::source::{SourceId, SourceMap};

pub struct DiagnosticRenderer<'a> {
    sources: &'a SourceMap,
    renderer: Renderer,
    line_indexes: RefCell<FxHashMap<SourceId, LineIndex>>,
}

impl<'a> DiagnosticRenderer<'a> {
    pub const GUTTER_STYLE: Style = DEFAULT_LINE_NUM_STYLE;

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

    pub fn render_note(&self, message: &str) -> String {
        self.renderer
            .render(&[Group::with_title(Level::NOTE.primary_title(message))])
    }

    pub fn render(&self, diagnostic: &Diagnostic) -> String {
        let (leading, trailing) = Self::split_helps(diagnostic.helps());
        let mut report = vec![self.main_group(diagnostic, leading)];
        report.extend(trailing.iter().map(|help| self.help_group(help)));
        self.renderer.render(&report)
    }

    fn split_helps(helps: &[Help]) -> (&[Help], &[Help]) {
        let first_suggestion = helps
            .iter()
            .position(|help| help.suggestion().is_some())
            .unwrap_or(helps.len());
        helps.split_at(first_suggestion)
    }

    fn help_group<'d>(&'d self, help: &'d Help) -> Group<'d> {
        self.suggestion_group(help)
            .unwrap_or_else(|| Group::with_title(Level::HELP.secondary_title(help.message())))
    }

    fn main_group<'d>(&'d self, diagnostic: &'d Diagnostic, helps: &'d [Help]) -> Group<'d> {
        let title = Self::level(diagnostic).primary_title(diagnostic.title());
        let title = match diagnostic.code() {
            Some(code) => title.id(code.as_str()),
            None => title,
        };
        let snippets = self
            .label_sources(diagnostic)
            .into_iter()
            .map(|source| self.labeled_snippet(diagnostic, source));
        let notes = diagnostic
            .notes()
            .iter()
            .map(|note| Level::NOTE.message(note.as_str()));
        let plain_helps = helps.iter().map(|help| Level::HELP.message(help.message()));
        title
            .elements(snippets)
            .elements(notes)
            .elements(plain_helps)
    }

    fn level(diagnostic: &Diagnostic) -> Level<'static> {
        if diagnostic.is_error() {
            Level::ERROR
        } else if diagnostic.is_note() {
            Level::NOTE
        } else {
            Level::WARNING
        }
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
    use crate::diagnostic::{Diagnostic, DiagnosticCode, Diagnostics, Label, Suggestion};
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

    #[test]
    fn helps_render_in_the_order_they_were_added() {
        let mut sources = SourceMap::default();
        let source = sources.add("sample.rslisp", "(prnt 1)");
        let span = Span::new(source, 1..5);
        let diagnostic = Diagnostic::error(
            DiagnosticCode::UNBOUND_NAME,
            "cannot find `prnt` in this scope",
            Label::new(span, "not found in this scope"),
        )
        .with_help("first help")
        .with_suggestion("a suggestion", Suggestion::new(span, "prn"))
        .with_help("last help");
        let rendered = DiagnosticRenderer::plain(&sources).render(&diagnostic);
        let first = rendered.find("first help").expect("the first help renders");
        let suggestion = rendered
            .find("a suggestion")
            .expect("the suggestion renders");
        let last = rendered.find("last help").expect("the last help renders");
        assert!(first < suggestion && suggestion < last, "{rendered}");
    }
}
