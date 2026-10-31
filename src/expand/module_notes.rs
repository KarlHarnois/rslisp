use crate::core::ModuleId;
use crate::reader::Form;

#[derive(Default)]
pub struct ModuleNotes {
    notes: Vec<ModuleNote>,
}

struct ModuleNote {
    module: ModuleId,
    key: String,
    note: Form,
}

#[derive(Clone, Copy)]
pub struct NotesCheckpoint(usize);

impl ModuleNotes {
    pub fn add(&mut self, module: ModuleId, key: &str, note: Form) {
        self.notes.push(ModuleNote {
            module,
            key: key.to_string(),
            note,
        });
    }

    pub fn of(&self, module: ModuleId, key: &str) -> Vec<Form> {
        self.notes
            .iter()
            .filter(|note| note.module == module && note.key == key)
            .map(|note| note.note.clone())
            .collect()
    }

    pub fn checkpoint(&self) -> NotesCheckpoint {
        NotesCheckpoint(self.notes.len())
    }

    pub fn roll_back(&mut self, checkpoint: NotesCheckpoint) {
        self.notes.truncate(checkpoint.0);
    }
}
