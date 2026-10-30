use super::Expander;
use super::form_budget::FormBudget;
use crate::diagnostic::{Diagnostic, DiagnosticCode, Help, Label};
use crate::scope::FastHashSet;

#[derive(PartialEq, Eq, Hash)]
pub(super) struct RepeatedDiagnostic {
    code: DiagnosticCode,
    title: String,
    labels: Vec<Label>,
    notes: Vec<String>,
    helps: Vec<Help>,
}

impl RepeatedDiagnostic {
    fn of(diagnostic: &Diagnostic) -> Self {
        Self {
            code: diagnostic.code().clone(),
            title: diagnostic.title().to_string(),
            labels: diagnostic.labels().cloned().collect(),
            notes: diagnostic.notes().to_vec(),
            helps: diagnostic.helps().to_vec(),
        }
    }
}

impl Expander<'_> {
    pub(super) fn within_top_level_form<T>(
        &mut self,
        expand: impl FnOnce(&mut Self) -> Result<T, Diagnostic>,
    ) -> Result<T, Diagnostic> {
        let first_recovered = self.recovered_diagnostics.len();
        let enclosing_budget = std::mem::replace(&mut self.form_budget, FormBudget::full());
        let expanded = expand(self);
        self.form_budget = enclosing_budget;
        let reported = expanded.as_ref().err().map(RepeatedDiagnostic::of);
        self.drop_repeated_recoveries(first_recovered, reported.into_iter().collect());
        expanded
    }

    pub(super) fn report_once(
        &mut self,
        diagnostic: Diagnostic,
        reported: &mut FastHashSet<RepeatedDiagnostic>,
    ) {
        if reported.insert(RepeatedDiagnostic::of(&diagnostic)) {
            self.diagnostics.push(diagnostic);
        }
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
