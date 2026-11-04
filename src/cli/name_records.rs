use std::rc::Rc;

use rustc_hash::FxHashMap;

use crate::core::Namespace;
use crate::diagnostic::MissingName;
use crate::expand::{BoundName, ExpandProblems};

#[derive(Clone, Default)]
pub struct NameRecords {
    records: FxHashMap<(Namespace, Rc<str>), NameRecord>,
}

#[derive(Clone)]
struct NameRecord {
    setter: String,
    cause: RecordCause,
}

#[derive(Clone)]
enum RecordCause {
    SetAside { reason: String },
    NoLongerDefined { input: String },
}

#[derive(Clone, Copy)]
pub enum Tense {
    Done,
    Pending,
}

impl NameRecords {
    pub fn set_aside(&mut self, name: &BoundName, setter: &str, reason: &str) {
        let cause = RecordCause::SetAside {
            reason: reason.to_string(),
        };
        self.record(name, setter, cause);
    }

    pub fn no_longer_defined(&mut self, name: &BoundName, input: &str, setter: &str) {
        let cause = RecordCause::NoLongerDefined {
            input: input.to_string(),
        };
        self.record(name, setter, cause);
    }

    pub fn absorb(&mut self, other: NameRecords) {
        self.records.extend(other.records);
    }

    pub fn forget<'a>(&mut self, bound: impl IntoIterator<Item = &'a BoundName>) {
        for name in bound {
            self.records
                .remove(&(name.namespace, Rc::clone(&name.name)));
        }
    }

    pub fn note(&self, missing: &MissingName, tense: Tense) -> Option<String> {
        let record = self
            .records
            .get(&(missing.namespace, Rc::clone(&missing.name)))?;
        Some(record.note(&missing.name, tense))
    }

    fn record(&mut self, name: &BoundName, setter: &str, cause: RecordCause) {
        let record = NameRecord {
            setter: setter.to_string(),
            cause,
        };
        self.records
            .insert((name.namespace, Rc::clone(&name.name)), record);
    }
}

impl NameRecord {
    fn note(&self, name: &str, tense: Tense) -> String {
        match (&self.cause, tense) {
            (RecordCause::SetAside { reason }, Tense::Done) => {
                ExpandProblems::set_aside_note(&self.setter, name, reason)
            }
            (RecordCause::SetAside { reason }, Tense::Pending) => {
                ExpandProblems::would_set_aside_note(&self.setter, name, reason)
            }
            (RecordCause::NoLongerDefined { input }, _) => {
                ExpandProblems::no_longer_defines_note(input, name, &self.setter)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{SourceId, Span};

    struct RecordFixture;

    impl RecordFixture {
        fn bound(name: &str, namespace: Namespace) -> BoundName {
            BoundName {
                name: name.into(),
                namespace,
                span: Span::new(SourceId::new(0), 0..1),
            }
        }

        fn missing(name: &str, namespace: Namespace) -> MissingName {
            MissingName {
                name: name.into(),
                namespace,
            }
        }

        fn set_aside_karl() -> NameRecords {
            let mut records = NameRecords::default();
            records.set_aside(
                &Self::bound("karl", Namespace::Value),
                "<input 4>",
                "missing field `email` in `user`",
            );
            records
        }
    }

    #[test]
    fn a_set_aside_name_notes_its_setter_and_reason() {
        let note = RecordFixture::set_aside_karl().note(
            &RecordFixture::missing("karl", Namespace::Value),
            Tense::Done,
        );
        assert_eq!(
            note.as_deref(),
            Some("`<input 4>` set aside `karl`: missing field `email` in `user`")
        );
    }

    #[test]
    fn a_pending_set_aside_name_notes_what_would_happen() {
        let note = RecordFixture::set_aside_karl().note(
            &RecordFixture::missing("karl", Namespace::Value),
            Tense::Pending,
        );
        assert_eq!(
            note.as_deref(),
            Some("`<input 4>` would set aside `karl`: missing field `email` in `user`")
        );
    }

    #[test]
    fn a_name_of_another_namespace_has_no_note() {
        let note = RecordFixture::set_aside_karl().note(
            &RecordFixture::missing("karl", Namespace::Type),
            Tense::Done,
        );
        assert_eq!(note, None);
    }

    #[test]
    fn a_name_no_longer_defined_notes_the_input_that_stopped() {
        let mut records = NameRecords::default();
        records.no_longer_defined(
            &RecordFixture::bound("foo", Namespace::Value),
            "<input 3>",
            "<input 4>",
        );
        let note = records.note(
            &RecordFixture::missing("foo", Namespace::Value),
            Tense::Done,
        );
        assert_eq!(
            note.as_deref(),
            Some("`<input 3>` no longer defines `foo` after `<input 4>`")
        );
    }

    #[test]
    fn a_forgotten_name_has_no_note() {
        let mut records = RecordFixture::set_aside_karl();
        records.forget([&RecordFixture::bound("karl", Namespace::Value)]);
        let missing = RecordFixture::missing("karl", Namespace::Value);
        assert_eq!(records.note(&missing, Tense::Done), None);
    }

    #[test]
    fn absorbed_records_note_their_names() {
        let mut records = NameRecords::default();
        records.absorb(RecordFixture::set_aside_karl());
        let missing = RecordFixture::missing("karl", Namespace::Value);
        assert!(records.note(&missing, Tense::Done).is_some());
    }
}
