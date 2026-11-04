use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextPosition {
    line: usize,
    column: usize,
}

impl TextPosition {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }

    pub fn offset_in(self, text: &str) -> Option<usize> {
        let line_start = self.line_start_in(text)?;
        let line_end = text[line_start..]
            .find('\n')
            .map_or(text.len(), |length| line_start + length);
        let offset = line_start + self.column.checked_sub(1)?;
        (offset <= line_end && text.is_char_boundary(offset)).then_some(offset)
    }

    fn line_start_in(self, text: &str) -> Option<usize> {
        let following_newlines = text.match_indices('\n').map(|(offset, _)| offset + 1);
        std::iter::once(0)
            .chain(following_newlines)
            .nth(self.line.checked_sub(1)?)
    }
}

impl fmt::Display for TextPosition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.line, self.column)
    }
}

#[cfg(test)]
mod tests {
    use super::TextPosition;

    struct PositionFixture;

    impl PositionFixture {
        const TEXT: &str = "(defn main ()\n  (prn \"☃\" 1))\n";

        fn offset(line: usize, column: usize) -> Option<usize> {
            TextPosition::new(line, column).offset_in(Self::TEXT)
        }
    }

    #[test]
    fn the_first_position_is_the_start_of_the_text() {
        assert_eq!(PositionFixture::offset(1, 1), Some(0));
    }

    #[test]
    fn a_column_counts_bytes_from_the_start_of_its_line() {
        assert_eq!(
            PositionFixture::offset(2, 3),
            PositionFixture::TEXT.find("(prn")
        );
        assert_eq!(
            PositionFixture::offset(2, 13),
            PositionFixture::TEXT.find(" 1")
        );
    }

    #[test]
    fn a_column_may_sit_at_the_end_of_its_line() {
        assert_eq!(
            PositionFixture::offset(1, 14),
            PositionFixture::TEXT.find('\n')
        );
    }

    #[test]
    fn a_column_past_the_end_of_its_line_has_no_offset() {
        assert_eq!(PositionFixture::offset(1, 15), None);
    }

    #[test]
    fn a_column_inside_a_character_has_no_offset() {
        assert_eq!(PositionFixture::offset(2, 10), None);
    }

    #[test]
    fn a_line_past_the_end_of_the_text_has_no_offset() {
        assert_eq!(PositionFixture::offset(4, 1), None);
    }

    #[test]
    fn line_and_column_zero_have_no_offset() {
        assert_eq!(PositionFixture::offset(0, 1), None);
        assert_eq!(PositionFixture::offset(1, 0), None);
    }
}
