use std::ops::Range;

pub struct LineIndex {
    starts: Vec<usize>,
    length: usize,
}

pub struct SourceWindow {
    first_line: usize,
    bytes: Range<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let following_newlines = text.match_indices('\n').map(|(offset, _)| offset + 1);
        Self {
            starts: std::iter::once(0).chain(following_newlines).collect(),
            length: text.len(),
        }
    }

    pub fn window_covering(&self, ranges: impl Iterator<Item = Range<usize>>) -> SourceWindow {
        let covered = ranges
            .reduce(|left, right| left.start.min(right.start)..left.end.max(right.end))
            .unwrap_or_default();
        self.window(covered)
    }

    pub fn window(&self, covered: Range<usize>) -> SourceWindow {
        let first = self.line_of(covered.start);
        let last = self.line_of(covered.end);
        SourceWindow {
            first_line: first + 1,
            bytes: self.starts[first]..self.end_of_line(last),
        }
    }

    fn line_of(&self, offset: usize) -> usize {
        self.starts.partition_point(|start| *start <= offset) - 1
    }

    fn end_of_line(&self, line: usize) -> usize {
        self.starts.get(line + 1).copied().unwrap_or(self.length)
    }
}

impl SourceWindow {
    pub fn first_line(&self) -> usize {
        self.first_line
    }

    pub fn text<'t>(&self, source: &'t str) -> &'t str {
        &source[self.bytes.clone()]
    }

    pub fn relative(&self, span: Range<usize>) -> Range<usize> {
        span.start - self.bytes.start..span.end - self.bytes.start
    }
}

#[cfg(test)]
mod tests {
    use super::LineIndex;

    const TEXT: &str = "(defn main ()\n  (do\n    leaf\n    1))\n";

    #[test]
    fn a_window_holds_only_the_covered_lines() {
        let leaf = TEXT.find("leaf").unwrap();
        let window = LineIndex::new(TEXT).window(leaf..leaf + 4);
        assert_eq!(window.first_line(), 3);
        assert_eq!(window.text(TEXT), "    leaf\n");
        assert_eq!(window.relative(leaf..leaf + 4), 4..8);
    }

    #[test]
    fn a_window_spans_every_line_between_its_ranges() {
        let ranges = [0..5, TEXT.find('1').unwrap()..TEXT.len() - 1];
        let window = LineIndex::new(TEXT).window_covering(ranges.into_iter());
        assert_eq!(window.first_line(), 1);
        assert_eq!(window.text(TEXT), TEXT);
    }

    #[test]
    fn a_window_at_the_end_of_the_text_is_the_last_line() {
        let window = LineIndex::new(TEXT).window(TEXT.len()..TEXT.len());
        assert_eq!(window.first_line(), 5);
        assert_eq!(window.text(TEXT), "");
    }
}
