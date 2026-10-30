use super::Expander;
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::scope::FastHashSet;
use crate::source::Span;

#[derive(PartialEq, Eq, Hash)]
struct RepeatedDiagnostic {
    code: DiagnosticCode,
    span: Span,
    title: String,
    notes: Vec<String>,
}

impl RepeatedDiagnostic {
    fn of(diagnostic: &Diagnostic) -> Self {
        Self {
            code: diagnostic.code().clone(),
            span: diagnostic.primary().span(),
            title: diagnostic.title().to_string(),
            notes: diagnostic.notes().to_vec(),
        }
    }
}

impl Expander<'_> {
    pub(super) fn within_top_level_form<T>(
        &mut self,
        expand: impl FnOnce(&mut Self) -> Result<T, Diagnostic>,
    ) -> Result<T, Diagnostic> {
        let first_recovered = self.recovered_diagnostics.len();
        let expanded = expand(self);
        let reported = expanded.as_ref().err().map(RepeatedDiagnostic::of);
        self.drop_repeated_recoveries(first_recovered, reported.into_iter().collect());
        expanded
    }

    fn drop_repeated_recoveries(
        &mut self,
        first_recovered: usize,
        mut reported: FastHashSet<RepeatedDiagnostic>,
    ) {
        let recovered = self.recovered_diagnostics.split_off(first_recovered);
        self.recovered_diagnostics.extend(
            recovered
                .into_iter()
                .filter(|diagnostic| reported.insert(RepeatedDiagnostic::of(diagnostic))),
        );
    }
}
