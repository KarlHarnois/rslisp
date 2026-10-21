use super::Diagnostic;

#[derive(Clone, Debug, Default)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    pub fn extend(&mut self, other: Diagnostics) {
        self.items.extend(other.items);
    }

    pub fn has_errors(&self) -> bool {
        !self.items.is_empty()
    }

    pub fn in_source_order(&self) -> Vec<&Diagnostic> {
        let mut ordered: Vec<&Diagnostic> = self.items.iter().collect();
        ordered.sort_by_key(|diagnostic| {
            let span = diagnostic.primary().span();
            (span.source().index(), span.range().start)
        });
        ordered
    }

    pub fn into_result<T>(self, value: T) -> Result<T, Diagnostics> {
        if self.has_errors() {
            Err(self)
        } else {
            Ok(value)
        }
    }
}

impl From<Diagnostic> for Diagnostics {
    fn from(diagnostic: Diagnostic) -> Self {
        Self {
            items: vec![diagnostic],
        }
    }
}
