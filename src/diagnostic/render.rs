use annotate_snippets::{AnnotationKind, Group, Level, Patch, Renderer, Snippet};

use super::{Diagnostic, Diagnostics, Help, Label, Severity, Suggestion};
use crate::source::{SourceId, SourceMap};

pub struct DiagnosticRenderer<'a> {
    sources: &'a SourceMap,
    renderer: Renderer,
}

impl<'a> DiagnosticRenderer<'a> {
    pub fn plain(sources: &'a SourceMap) -> Self {
        Self {
            sources,
            renderer: Renderer::plain(),
        }
    }

    pub fn styled(sources: &'a SourceMap) -> Self {
        Self {
            sources,
            renderer: Renderer::styled(),
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
        let title = Self::level(diagnostic.severity())
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
        let annotations = diagnostic
            .labels()
            .filter(|label| label.span().source() == source)
            .map(|label| Self::annotation(label, label.span() == primary_span));
        self.snippet(source).annotations(annotations)
    }

    fn annotation(label: &Label, is_primary: bool) -> annotate_snippets::Annotation<'_> {
        let kind = if is_primary {
            AnnotationKind::Primary
        } else {
            AnnotationKind::Context
        };
        kind.span(label.span().range()).label(label.message())
    }

    fn suggestion_group<'d>(&'d self, help: &'d Help) -> Option<Group<'d>> {
        let suggestion = help.suggestion()?;
        Some(
            Group::with_title(Level::HELP.secondary_title(help.message()))
                .element(self.patched_snippet(suggestion)),
        )
    }

    fn patched_snippet<'d>(&'d self, suggestion: &'d Suggestion) -> Snippet<'d, Patch<'d>> {
        let file = self.sources.file(suggestion.span().source());
        Snippet::source(file.text())
            .path(file.name())
            .patch(Patch::new(
                suggestion.span().range(),
                suggestion.replacement(),
            ))
    }

    fn snippet<'d, T: Clone>(&'d self, source: SourceId) -> Snippet<'d, T> {
        let file = self.sources.file(source);
        Snippet::source(file.text()).path(file.name())
    }

    fn level(severity: Severity) -> Level<'static> {
        match severity {
            Severity::Error => Level::ERROR,
            Severity::Warning => Level::WARNING,
        }
    }
}
