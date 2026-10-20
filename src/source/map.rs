use super::{SourceFile, SourceId};

#[derive(Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn add(&mut self, name: impl Into<String>, text: impl Into<String>) -> SourceId {
        let id = SourceId::new(self.files.len());
        self.files.push(SourceFile::new(id, name.into(), text.into()));
        id
    }

    pub fn file(&self, id: SourceId) -> &SourceFile {
        &self.files[id.index()]
    }
}
