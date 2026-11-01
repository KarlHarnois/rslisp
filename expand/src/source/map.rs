use super::{SourceFile, SourceId};

#[derive(Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn add(&mut self, name: impl Into<String>, text: impl Into<String>) -> SourceId {
        let id = self.next_id();
        self.files
            .push(SourceFile::new(id, name.into(), text.into()));
        id
    }

    pub fn next_id(&self) -> SourceId {
        SourceId::new(self.files.len())
    }

    pub fn file(&self, id: SourceId) -> &SourceFile {
        &self.files[id.index()]
    }
}
