use super::SourceId;

pub struct SourceFile {
    id: SourceId,
    name: String,
    text: String,
}

impl SourceFile {
    pub fn new(id: SourceId, name: String, text: String) -> Self {
        Self { id, name, text }
    }

    pub fn id(&self) -> SourceId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}
