use crate::diagnostic::{Diagnostic, DiagnosticCode, Label};
use crate::scope::FastHashMap;

#[derive(Default)]
pub struct DistinctDiagnostics {
    kept: Vec<Diagnostic>,
    positions: FastHashMap<DiagnosticKey, usize>,
}

#[derive(PartialEq, Eq, Hash)]
struct DiagnosticKey {
    code: DiagnosticCode,
    title: String,
    labels: Vec<Label>,
    notes: Vec<String>,
}

impl DistinctDiagnostics {
    pub fn of(diagnostics: impl IntoIterator<Item = Diagnostic>) -> Self {
        let mut distinct = Self::default();
        for diagnostic in diagnostics {
            distinct.add(diagnostic);
        }
        distinct
    }

    pub fn add(&mut self, diagnostic: Diagnostic) {
        let key = DiagnosticKey::of(&diagnostic);
        match self.positions.get(&key) {
            Some(&index) => self.keep_more_helpful(index, diagnostic),
            None => {
                self.positions.insert(key, self.kept.len());
                self.kept.push(diagnostic);
            }
        }
    }

    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.kept
    }

    fn keep_more_helpful(&mut self, index: usize, diagnostic: Diagnostic) {
        if diagnostic.helps().len() > self.kept[index].helps().len() {
            self.kept[index] = diagnostic;
        }
    }
}

impl DiagnosticKey {
    fn of(diagnostic: &Diagnostic) -> Self {
        Self {
            code: diagnostic.code().clone(),
            title: diagnostic.title().to_string(),
            labels: diagnostic.labels().cloned().collect(),
            notes: diagnostic.notes().to_vec(),
        }
    }
}
