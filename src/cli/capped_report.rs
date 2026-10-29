use crate::diagnostic::{Counted, DiagnosticRenderer, Diagnostics};

pub struct CappedReport<'a> {
    renderer: DiagnosticRenderer<'a>,
}

impl<'a> CappedReport<'a> {
    const MAXIMUM_PRINTED_DIAGNOSTICS: usize = 20;
    const SEPARATOR: &'static str = "\n\n";

    pub fn new(renderer: DiagnosticRenderer<'a>) -> Self {
        Self { renderer }
    }

    pub fn render(&self, diagnostics: &Diagnostics) -> String {
        let ordered = diagnostics.in_source_order();
        let mut sections: Vec<String> = ordered
            .iter()
            .take(Self::MAXIMUM_PRINTED_DIAGNOSTICS)
            .map(|diagnostic| self.renderer.render(diagnostic))
            .collect();
        let hidden = ordered
            .len()
            .saturating_sub(Self::MAXIMUM_PRINTED_DIAGNOSTICS);
        if hidden > 0 {
            sections.push(Self::summary(hidden));
        }
        sections.join(Self::SEPARATOR)
    }

    fn summary(hidden: usize) -> String {
        format!(
            "note: {} {} not shown",
            Counted::noun(hidden, "more diagnostic"),
            Counted::was_or_were(hidden)
        )
    }
}
