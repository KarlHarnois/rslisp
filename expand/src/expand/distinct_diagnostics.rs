use rustc_hash::FxHashMap;

use crate::diagnostic::{Diagnostic, DiagnosticCode, Label};

#[derive(Default)]
pub struct DistinctDiagnostics {
    kept: Vec<Diagnostic>,
    positions: FxHashMap<DiagnosticKey, usize>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Help;
    use crate::source::{SourceId, Span};

    struct DistinctFixture;

    impl DistinctFixture {
        fn unbound() -> Diagnostic {
            let span = Span::new(SourceId::new(0), 0..4);
            Diagnostic::error(
                DiagnosticCode::UNBOUND_NAME,
                "cannot find `name` in this scope",
                Label::new(span, "not found in this scope"),
            )
        }

        fn kept(diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
            DistinctDiagnostics::of(diagnostics).into_diagnostics()
        }

        fn help_messages(diagnostics: &[Diagnostic]) -> Vec<&str> {
            diagnostics
                .iter()
                .flat_map(Diagnostic::helps)
                .map(Help::message)
                .collect()
        }
    }

    #[test]
    fn keeps_the_first_of_two_copies_with_as_many_helps() {
        let first = DistinctFixture::unbound().with_help("rename it");
        let second = DistinctFixture::unbound().with_help("import it");
        let kept = DistinctFixture::kept(vec![first, second]);
        assert_eq!(DistinctFixture::help_messages(&kept), vec!["rename it"]);
    }

    #[test]
    fn keeps_both_copies_whose_notes_differ() {
        let first = DistinctFixture::unbound().with_note("in this expansion of `left`");
        let second = DistinctFixture::unbound().with_note("in this expansion of `right`");
        assert_eq!(DistinctFixture::kept(vec![first, second]).len(), 2);
    }
}
