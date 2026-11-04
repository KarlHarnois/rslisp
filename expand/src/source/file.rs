use std::ops::Range;

use super::SourceId;

pub struct SourceFile {
    id: SourceId,
    name: String,
    text: String,
    excerpt: Range<usize>,
}

impl SourceFile {
    pub fn new(id: SourceId, name: String, text: String) -> Self {
        let whole = 0..text.len();
        Self::excerpt(id, name, text, whole)
    }

    pub fn excerpt(id: SourceId, name: String, text: String, excerpt: Range<usize>) -> Self {
        Self {
            id,
            name,
            text,
            excerpt,
        }
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

    pub fn excerpt_range(&self) -> Range<usize> {
        self.excerpt.clone()
    }
}
