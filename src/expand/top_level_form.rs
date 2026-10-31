use super::Expander;
use super::distinct_diagnostics::DistinctDiagnostics;
use crate::diagnostic::Diagnostic;

impl Expander<'_> {
    pub(super) fn within_top_level_form<T>(
        &mut self,
        expand: impl FnOnce(&mut Self) -> Result<T, Diagnostic>,
    ) -> Result<T, Diagnostic> {
        let first_recovered = self.recovered_diagnostics.len();
        let enclosing_budget = self.form_budget.refilled();
        let expanded = expand(self);
        self.form_budget = enclosing_budget;
        let recovered = self.recovered_diagnostics.split_off(first_recovered);
        match expanded {
            Ok(value) => {
                self.recover_distinct(recovered);
                Ok(value)
            }
            Err(error) => Err(self.recover_distinct_beside(error, recovered)),
        }
    }

    fn recover_distinct(&mut self, recovered: Vec<Diagnostic>) {
        let distinct = DistinctDiagnostics::of(recovered);
        self.recovered_diagnostics
            .extend(distinct.into_diagnostics());
    }

    fn recover_distinct_beside(
        &mut self,
        error: Diagnostic,
        recovered: Vec<Diagnostic>,
    ) -> Diagnostic {
        let with_error = std::iter::once(error).chain(recovered);
        let mut distinct = DistinctDiagnostics::of(with_error).into_diagnostics();
        let most_helpful_error = distinct.remove(0);
        self.recovered_diagnostics.extend(distinct);
        most_helpful_error
    }
}
