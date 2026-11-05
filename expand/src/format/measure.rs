use super::block::Block;
use super::layout::{LaidOut, LayoutEngine};
use super::syntax_tree::ListNode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Measure {
    line_count: usize,
    closed_overflow: usize,
    end_column: usize,
    ends_with_comment: bool,
}

impl LaidOut for Measure {
    fn text(text: &str, column: usize) -> Self {
        let widths: Vec<usize> = text.split('\n').map(|line| line.chars().count()).collect();
        let (last, earlier) = widths.split_last().expect("a split text has a line");
        let absolute_end =
            |index: usize, width: usize| if index == 0 { column + width } else { width };
        Self {
            line_count: widths.len(),
            closed_overflow: earlier
                .iter()
                .enumerate()
                .map(|(index, width)| Block::overflow_of(absolute_end(index, *width)))
                .sum(),
            end_column: absolute_end(earlier.len(), *last),
            ends_with_comment: false,
        }
    }

    fn shifted_string(text: &str, shift: isize, column: usize) -> Self {
        let mut rendered = String::new();
        Block::shifted_string(text, shift, column).render_into(&mut rendered);
        Self::text(&rendered, column)
    }

    fn list(
        engine: &mut LayoutEngine<'_>,
        list: &ListNode,
        column: usize,
        trailing: usize,
    ) -> Self {
        engine.measure_list(list, column, trailing)
    }

    fn line_count(&self) -> usize {
        self.line_count
    }

    fn overflow(&self) -> usize {
        self.closed_overflow + Block::overflow_of(self.end_column)
    }

    fn end_column(&self) -> usize {
        self.end_column
    }

    fn ends_with_comment(&self) -> bool {
        self.ends_with_comment
    }

    fn append(&mut self, text: &str) {
        self.end_column += text.chars().count();
    }

    fn join(&mut self, _separator: &str, other: Self) {
        self.line_count += other.line_count - 1;
        self.closed_overflow += other.closed_overflow;
        self.end_column = other.end_column;
        self.ends_with_comment = other.ends_with_comment;
    }

    fn push_indented(&mut self, _indent: usize, other: Self) {
        self.close_line();
        self.line_count += other.line_count - 1;
        self.closed_overflow += other.closed_overflow;
        self.end_column = other.end_column;
        self.ends_with_comment = other.ends_with_comment;
    }

    fn push_blank(&mut self) {
        self.close_line();
        self.end_column = 0;
        self.ends_with_comment = false;
    }

    fn add_trailing_comment(&mut self, _comment: &str) {
        self.ends_with_comment = true;
    }
}

impl Measure {
    fn close_line(&mut self) {
        self.closed_overflow += Block::overflow_of(self.end_column);
        self.line_count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::Measure;
    use crate::format::block::Block;
    use crate::format::layout::LaidOut;

    struct Agreement {
        block: Block,
        measure: Measure,
    }

    impl Agreement {
        fn of_text(text: &str, column: usize) -> Self {
            Self {
                block: Block::text(text, column),
                measure: Measure::text(text, column),
            }
        }

        fn push_indented(&mut self, indent: usize, text: &str) {
            self.block.push_indented(indent, Block::text(text, indent));
            self.measure
                .push_indented(indent, Measure::text(text, indent));
        }

        fn join(&mut self, text: &str) {
            let column = self.block.end_column() + 1;
            self.block.join(" ", Block::text(text, column));
            self.measure.join(" ", Measure::text(text, column));
        }

        fn assert_agrees(&self) {
            assert_eq!(
                (
                    self.block.line_count(),
                    self.block.overflow(),
                    self.block.end_column()
                ),
                (
                    self.measure.line_count(),
                    self.measure.overflow(),
                    self.measure.end_column()
                )
            );
        }
    }

    #[test]
    fn a_measure_counts_what_its_block_lays_out() {
        let long = "y".repeat(Block::WIDTH);
        let mut agreement = Agreement::of_text("(head", 70);
        agreement.join(&format!("\"{long}\nend\""));
        agreement.push_indented(4, &long);
        agreement.join("tail");
        agreement.assert_agrees();
    }
}
