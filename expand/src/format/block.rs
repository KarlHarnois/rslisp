use super::layout::{LaidOut, LayoutEngine};
use super::syntax_tree::ListNode;

#[derive(Clone, Debug)]
struct Line {
    text: String,
    comment: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Block {
    column: usize,
    first: Line,
    later: Vec<Line>,
}

impl LaidOut for Block {
    fn text(text: &str, column: usize) -> Self {
        let mut lines = text.split('\n');
        Self {
            column,
            first: Line::plain(lines.next().unwrap_or_default()),
            later: lines.map(Line::plain).collect(),
        }
    }

    fn shifted_string(text: &str, shift: isize, column: usize) -> Self {
        let mut block = Self::text(text, column);
        for line in &mut block.later {
            line.shift(shift);
        }
        block
    }

    fn list(
        engine: &mut LayoutEngine<'_>,
        list: &ListNode,
        column: usize,
        trailing: usize,
    ) -> Self {
        engine.render_list(list, column, trailing)
    }

    fn line_count(&self) -> usize {
        1 + self.later.len()
    }

    fn overflow(&self) -> usize {
        let first = self.column + self.first.width();
        self.later
            .iter()
            .map(Line::width)
            .chain([first])
            .map(Self::overflow_of)
            .sum()
    }

    fn end_column(&self) -> usize {
        match self.later.last() {
            Some(line) => line.width(),
            None => self.column + self.first.width(),
        }
    }

    fn ends_with_comment(&self) -> bool {
        self.last().comment.is_some()
    }

    fn append(&mut self, text: &str) {
        self.last_mut().text.push_str(text);
    }

    fn join(&mut self, separator: &str, other: Block) {
        let last = self.last_mut();
        last.text.push_str(separator);
        last.text.push_str(&other.first.text);
        last.comment = other.first.comment;
        self.later.extend(other.later);
    }

    fn push_indented(&mut self, indent: usize, other: Block) {
        self.later.push(Line::plain(&Self::spaces(indent)));
        self.join("", other);
    }

    fn push_blank(&mut self) {
        self.later.push(Line::plain(""));
    }

    fn add_trailing_comment(&mut self, comment: &str) {
        self.last_mut().comment = Some(comment.to_string());
    }
}

impl Block {
    pub const WIDTH: usize = 80;
    pub const FILL_WIDTH: usize = 60;

    pub fn overflow_of(width: usize) -> usize {
        width.saturating_sub(Self::WIDTH)
    }

    pub fn render_into(&self, output: &mut String) {
        self.first.render_into(output);
        for line in &self.later {
            output.push('\n');
            line.render_into(output);
        }
    }

    fn last(&self) -> &Line {
        self.later.last().unwrap_or(&self.first)
    }

    fn last_mut(&mut self) -> &mut Line {
        self.later.last_mut().unwrap_or(&mut self.first)
    }

    fn spaces(count: usize) -> String {
        " ".repeat(count)
    }
}

impl Line {
    fn plain(text: &str) -> Self {
        Self {
            text: text.to_string(),
            comment: None,
        }
    }

    fn width(&self) -> usize {
        self.text.chars().count()
    }

    fn shift(&mut self, shift: isize) {
        if self.text.is_empty() {
            return;
        }
        let indentation = self.text.len() - self.text.trim_start_matches(' ').len();
        let shifted = indentation.saturating_add_signed(shift);
        self.text = format!("{}{}", Block::spaces(shifted), &self.text[indentation..]);
    }

    fn render_into(&self, output: &mut String) {
        output.push_str(&self.text);
        if let Some(comment) = &self.comment {
            output.push(' ');
            output.push_str(comment);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Block;
    use crate::format::layout::LaidOut;

    struct BlockFixture;

    impl BlockFixture {
        fn rendered(block: &Block) -> String {
            let mut output = String::new();
            block.render_into(&mut output);
            output
        }
    }

    #[test]
    fn a_closing_parenthesis_goes_before_a_trailing_comment() {
        let mut block = Block::text("b", 0);
        block.add_trailing_comment("; why");
        block.append(")");
        assert_eq!(BlockFixture::rendered(&block), "b) ; why");
    }

    #[test]
    fn a_shift_moves_each_later_line_and_leaves_blank_lines_empty() {
        let block = Block::shifted_string("\"a\n\n  b\"", 2, 0);
        assert_eq!(BlockFixture::rendered(&block), "\"a\n\n    b\"");
    }

    #[test]
    fn a_negative_shift_removes_at_most_the_indentation() {
        let block = Block::shifted_string("\"a\n b\"", -3, 0);
        assert_eq!(BlockFixture::rendered(&block), "\"a\nb\"");
    }

    #[test]
    fn overflow_counts_the_columns_past_the_width() {
        let block = Block::text(&"x".repeat(Block::WIDTH), 2);
        assert_eq!(block.overflow(), 2);
    }
}
