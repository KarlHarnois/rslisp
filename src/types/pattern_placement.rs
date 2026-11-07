use std::rc::Rc;

use super::problems::TypeProblems;
use crate::core::{FieldLabel, Pattern, PatternArguments, PatternKind};
use crate::diagnostic::{Diagnostic, SimilarName};
use crate::source::Span;

pub enum PatternFields<'a> {
    Named(Vec<&'a str>),
    Positional(usize),
}

impl PatternFields<'_> {
    fn count(&self) -> usize {
        match self {
            Self::Named(names) => names.len(),
            Self::Positional(count) => *count,
        }
    }
}

pub struct PatternPlacement<'a> {
    written: &'a PatternArguments,
    skipped_field: Pattern,
    span: Span,
}

impl<'a> PatternPlacement<'a> {
    pub fn new(written: &'a PatternArguments, span: Span) -> Self {
        Self {
            written,
            skipped_field: Pattern::new(PatternKind::Wildcard, span),
            span,
        }
    }

    pub fn place(
        &self,
        owner: &str,
        fields: &PatternFields<'_>,
    ) -> Result<Vec<&Pattern>, Diagnostic> {
        self.reject_mixed_forms(owner, fields)?;
        match (self.written.labeled().next(), fields) {
            (None, _) => self.by_position(owner, fields.count()),
            (Some(_), PatternFields::Named(names)) => self.by_label(owner, names),
            (Some((label, _)), PatternFields::Positional(count)) => Err(
                TypeProblems::label_on_positional_payload(owner, label, *count),
            ),
        }
    }

    fn reject_mixed_forms(
        &self,
        owner: &str,
        fields: &PatternFields<'_>,
    ) -> Result<(), Diagnostic> {
        let (Some((label, _)), Some(positional)) = (
            self.written.labeled().next(),
            self.written.positional().next(),
        ) else {
            return Ok(());
        };
        Err(match fields {
            PatternFields::Named(_) => {
                TypeProblems::mixed_named_pattern(owner, label.span, positional.span)
            }
            PatternFields::Positional(count) => {
                TypeProblems::mixed_positional_pattern(owner, label.span, positional.span, *count)
            }
        })
    }

    fn by_position(&self, owner: &str, expected: usize) -> Result<Vec<&Pattern>, Diagnostic> {
        let found = self.written.positional().count();
        if expected != found {
            return Err(TypeProblems::pattern_count(
                owner, expected, found, self.span,
            ));
        }
        Ok(self.written.positional().collect())
    }

    fn by_label(&self, owner: &str, names: &[&str]) -> Result<Vec<&Pattern>, Diagnostic> {
        let mut slots: Vec<Option<(&FieldLabel, &Pattern)>> = vec![None; names.len()];
        for (label, pattern) in self.written.labeled() {
            let Some(index) = names.iter().position(|name| *name == &*label.name) else {
                return Err(Self::unknown_field(owner, label, names));
            };
            if let Some((first, _)) = slots[index] {
                return Err(TypeProblems::repeated_pattern_field(
                    owner, label, first.span,
                ));
            }
            let Some(pattern) = pattern else {
                return Err(TypeProblems::keyword_without_pattern(label));
            };
            slots[index] = Some((label, pattern));
        }
        Ok(slots
            .into_iter()
            .map(|slot| slot.map_or(&self.skipped_field, |(_, pattern)| pattern))
            .collect())
    }

    fn unknown_field(owner: &str, label: &FieldLabel, names: &[&str]) -> Diagnostic {
        let names: Vec<Rc<str>> = names.iter().map(|name| Rc::from(*name)).collect();
        TypeProblems::unknown_pattern_field(owner, label, SimilarName::closest(&label.name, &names))
    }
}
