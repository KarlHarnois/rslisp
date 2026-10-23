use rslisp::diagnostic::{Diagnostic, DiagnosticRenderer, Diagnostics};
use rslisp::expand::ExpandProblems;
use rslisp::source::{SourceId, SourceMap, Span};

struct DiagnosticFixture {
    sources: SourceMap,
}

impl DiagnosticFixture {
    const SOURCE_NAME: &'static str = "sample.rslisp";
    const SOURCE_TEXT: &'static str = "origin";

    fn new() -> Self {
        let mut sources = SourceMap::default();
        sources.add(Self::SOURCE_NAME, Self::SOURCE_TEXT);
        Self { sources }
    }

    fn render(&self, diagnostic: Diagnostic) -> String {
        let mut diagnostics = Diagnostics::default();
        diagnostics.push(diagnostic);
        DiagnosticRenderer::plain(&self.sources).render_all(&diagnostics)
    }

    fn span(&self) -> Span {
        Span::new(SourceId::new(0), 0..Self::SOURCE_TEXT.len())
    }
}

#[test]
fn not_at_compile_time_has_a_snapshot() {
    let fixture = DiagnosticFixture::new();
    let diagnostic = ExpandProblems::not_at_compile_time("origin", fixture.span());
    insta::assert_snapshot!(fixture.render(diagnostic));
}
