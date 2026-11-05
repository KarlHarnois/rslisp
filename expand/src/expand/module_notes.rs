use crate::core::ModuleId;
use crate::reader::Form;

use super::saved_notes::{NotesKey, NotesTable, SavedNotes};

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct ModuleNotes {
    saved: SavedNotes,
    notes: NotesTable,
    added: Vec<NotesKey>,
}

#[derive(Clone, Copy)]
pub struct NotesCheckpoint(usize);

impl ModuleNotes {
    pub fn add(&mut self, module: ModuleId, key: &str, note: Form) {
        let notes_key = (module, key.to_string());
        self.notes.entry(notes_key.clone()).or_default().push(note);
        self.added.push(notes_key);
    }

    pub fn saved(&self) -> Self {
        Self {
            saved: SavedNotes::encode(&self.notes),
            ..Self::default()
        }
    }

    pub fn of(&self, module: ModuleId, key: &str) -> Vec<Form> {
        self.notes
            .get(&(module, key.to_string()))
            .or_else(|| self.saved.of(module, key))
            .cloned()
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub fn decoded_no_saved_note(&self) -> bool {
        self.saved.decoded_nothing()
    }

    pub fn checkpoint(&self) -> NotesCheckpoint {
        NotesCheckpoint(self.added.len())
    }

    pub fn roll_back(&mut self, checkpoint: NotesCheckpoint) {
        for notes_key in self.added.split_off(checkpoint.0).into_iter().rev() {
            self.notes
                .get_mut(&notes_key)
                .expect("every added key holds its notes")
                .pop();
        }
    }
}
