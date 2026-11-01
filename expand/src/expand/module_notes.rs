use crate::core::ModuleId;
use crate::reader::Form;
use crate::scope::FastHashMap;

type NotesKey = (ModuleId, String);

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct ModuleNotes {
    notes: FastHashMap<NotesKey, Vec<Form>>,
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

    pub fn of(&self, module: ModuleId, key: &str) -> Vec<Form> {
        self.notes
            .get(&(module, key.to_string()))
            .cloned()
            .unwrap_or_default()
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
