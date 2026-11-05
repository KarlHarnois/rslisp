use std::cell::OnceCell;
use std::fmt;

use rustc_hash::{FxHashMap, FxHashSet};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::core::ModuleId;
use crate::reader::Form;

pub type NotesKey = (ModuleId, String);
pub type NotesTable = FxHashMap<NotesKey, Vec<Form>>;

#[derive(Default, Serialize, Deserialize)]
pub struct SavedNotes {
    modules: FxHashSet<ModuleId>,
    encoded: EncodedTable,
    #[serde(skip)]
    decoded: OnceCell<NotesTable>,
}

impl SavedNotes {
    pub fn encode(table: &NotesTable) -> Self {
        Self {
            modules: table.keys().map(|(module, _)| *module).collect(),
            encoded: EncodedTable(bincode::serialize(table).expect("every note serializes")),
            decoded: OnceCell::new(),
        }
    }

    pub fn of(&self, module: ModuleId, key: &str) -> Option<&Vec<Form>> {
        if !self.modules.contains(&module) {
            return None;
        }
        self.decoded().get(&(module, key.to_string()))
    }

    #[cfg(test)]
    pub fn decoded_nothing(&self) -> bool {
        self.decoded.get().is_none()
    }

    fn decoded(&self) -> &NotesTable {
        self.decoded
            .get_or_init(|| bincode::deserialize(&self.encoded.0).expect("the saved notes decode"))
    }
}

#[derive(Default)]
struct EncodedTable(Vec<u8>);

impl Serialize for EncodedTable {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for EncodedTable {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_byte_buf(EncodedTableVisitor)
    }
}

struct EncodedTableVisitor;

impl Visitor<'_> for EncodedTableVisitor {
    type Value = EncodedTable;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the encoded notes table")
    }

    fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<EncodedTable, E> {
        Ok(EncodedTable(bytes.to_vec()))
    }

    fn visit_byte_buf<E: de::Error>(self, bytes: Vec<u8>) -> Result<EncodedTable, E> {
        Ok(EncodedTable(bytes))
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::reader::FormKind;
    use crate::source::{SourceId, Span};

    struct SavedTable {
        noted_module: ModuleId,
        other_module: ModuleId,
    }

    impl SavedTable {
        const KEY: &str = "docstrings";
        const NOTE: &str = "A saved note.";

        fn new() -> Self {
            Self {
                noted_module: ModuleId::new(1),
                other_module: ModuleId::new(2),
            }
        }

        fn saved(&self) -> SavedNotes {
            let mut table = NotesTable::default();
            table.insert(
                (self.noted_module, Self::KEY.to_string()),
                vec![Self::note()],
            );
            let bytes =
                bincode::serialize(&SavedNotes::encode(&table)).expect("saved notes serialize");
            bincode::deserialize(&bytes).expect("saved notes deserialize")
        }

        fn note() -> Form {
            Form::new(
                FormKind::String(Rc::from(Self::NOTE)),
                Span::new(SourceId::new(0), 0..0),
            )
        }
    }

    #[test]
    fn a_loaded_table_decodes_no_note_before_a_read() {
        let saved = SavedTable::new().saved();
        assert!(saved.decoded.get().is_none());
    }

    #[test]
    fn a_read_of_a_noted_module_decodes_its_notes() {
        let table = SavedTable::new();
        let saved = table.saved();
        let notes = saved.of(table.noted_module, SavedTable::KEY);
        assert_eq!(notes.map(Vec::len), Some(1));
    }

    #[test]
    fn a_read_of_a_module_without_saved_notes_decodes_nothing() {
        let table = SavedTable::new();
        let saved = table.saved();
        assert!(saved.of(table.other_module, SavedTable::KEY).is_none());
        assert!(saved.decoded.get().is_none());
    }
}
