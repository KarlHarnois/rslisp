use rustc_hash::FxHashSet;

use super::ty::TypeVar;
use crate::diagnostic::{Diagnostic, Diagnostics};

struct MissingAnnotation {
    open_vars: Vec<TypeVar>,
    diagnostic: Diagnostic,
}

#[derive(Default)]
pub struct MissingAnnotations {
    reports: Vec<MissingAnnotation>,
}

impl MissingAnnotations {
    pub fn defer(&mut self, open_vars: Vec<TypeVar>, diagnostic: Diagnostic) {
        self.reports.push(MissingAnnotation {
            open_vars,
            diagnostic,
        });
    }

    pub fn one_per_variable(self, roots_of: impl Fn(TypeVar) -> Vec<TypeVar>) -> Diagnostics {
        let mut reported_roots = FxHashSet::default();
        let mut diagnostics = Diagnostics::default();
        for report in self.reports {
            let roots = report.open_vars.iter().flat_map(|var| roots_of(*var));
            let new_roots = roots.filter(|root| reported_roots.insert(*root));
            if new_roots.count() > 0 {
                diagnostics.push(report.diagnostic);
            }
        }
        diagnostics
    }
}
