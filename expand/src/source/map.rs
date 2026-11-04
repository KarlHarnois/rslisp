use std::ops::Range;

use super::{SourceFile, SourceId, TextPosition};

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

    pub fn add_inside(
        &mut self,
        enclosing: SourceId,
        position: TextPosition,
        text: &str,
    ) -> Option<SourceId> {
        let enclosing_text = self.file(enclosing).text();
        let start = position.offset_in(enclosing_text)?;
        let range = start..start + text.len();
        let holds_text = enclosing_text.get(range.clone()) == Some(text);
        holds_text.then(|| self.add_excerpt(enclosing, range))
    }

    pub fn next_id(&self) -> SourceId {
        SourceId::new(self.files.len())
    }

    pub fn file(&self, id: SourceId) -> &SourceFile {
        &self.files[id.index()]
    }

    fn add_excerpt(&mut self, enclosing: SourceId, range: Range<usize>) -> SourceId {
        let id = self.next_id();
        let enclosing = self.file(enclosing);
        let excerpt = SourceFile::excerpt(
            id,
            enclosing.name().to_string(),
            enclosing.text().to_string(),
            range,
        );
        self.files.push(excerpt);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::SourceMap;
    use crate::source::{SourceId, TextPosition};

    struct InsideFixture {
        sources: SourceMap,
        enclosing: SourceId,
    }

    impl InsideFixture {
        const MODULE: &str = "geometry.rslisp";
        const TEXT: &str = "(defn area (w h) (* w h))\n(area 2 3)\n(area 4 5)\n";
        const SECOND_LINE: &str = "(area 2 3)";

        fn new() -> Self {
            let mut sources = SourceMap::default();
            let enclosing = sources.add(Self::MODULE, Self::TEXT);
            Self { sources, enclosing }
        }

        fn add(&mut self, position: TextPosition, text: &str) -> Option<SourceId> {
            self.sources.add_inside(self.enclosing, position, text)
        }

        fn second_line() -> TextPosition {
            TextPosition::new(2, 1)
        }
    }

    #[test]
    fn a_source_inside_another_starts_at_the_offset_of_its_position() {
        let mut fixture = InsideFixture::new();
        let inside = fixture
            .add(InsideFixture::second_line(), InsideFixture::SECOND_LINE)
            .expect("the text sits at that position");
        let file = fixture.sources.file(inside);
        let start = InsideFixture::TEXT
            .find(InsideFixture::SECOND_LINE)
            .expect("the text holds the line");
        assert_eq!(file.excerpt_range().start, start);
        assert_eq!(
            &file.text()[file.excerpt_range()],
            InsideFixture::SECOND_LINE
        );
        assert_eq!(file.text(), InsideFixture::TEXT);
        assert_eq!(file.name(), InsideFixture::MODULE);
    }

    #[test]
    fn a_source_inside_another_must_be_its_text_at_that_position() {
        let mut fixture = InsideFixture::new();
        assert_eq!(
            fixture.add(InsideFixture::second_line(), "(area 4 5)"),
            None
        );
    }

    #[test]
    fn a_source_inside_another_must_end_inside_its_text() {
        let mut fixture = InsideFixture::new();
        let past_the_end = format!("{}(", InsideFixture::TEXT);
        assert_eq!(fixture.add(TextPosition::new(1, 1), &past_the_end), None);
    }

    #[test]
    fn a_source_inside_another_needs_a_position_of_its_text() {
        let mut fixture = InsideFixture::new();
        assert_eq!(fixture.add(TextPosition::new(9, 1), ""), None);
    }
}
