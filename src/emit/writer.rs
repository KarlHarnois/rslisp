use std::ops::Range;

use super::fragment::Fragment;
use crate::source::Span;

#[derive(Default)]
pub struct SpanMap {
    entries: Vec<(Range<usize>, Span)>,
}

impl SpanMap {
    pub fn source_of(&self, rust: Range<usize>) -> Option<Span> {
        self.entries
            .iter()
            .filter(|(range, _)| range.start <= rust.start && rust.end <= range.end)
            .min_by_key(|(range, _)| range.end - range.start)
            .map(|(_, span)| *span)
    }

    pub fn moved(&self, boundaries: &[usize]) -> Self {
        let entries = self
            .entries
            .iter()
            .filter_map(|(range, span)| {
                let start = *boundaries.get(range.start)?;
                let end = *boundaries.get(range.end)?;
                (start <= end).then_some((start..end, *span))
            })
            .collect();
        Self { entries }
    }
}

#[derive(Default)]
pub struct RustWriter {
    output: String,
    indentation: usize,
    at_line_start: bool,
    span_map: SpanMap,
}

impl RustWriter {
    const INDENT: &str = "    ";

    pub fn write(&mut self, fragments: &[Fragment]) {
        for fragment in fragments {
            match fragment {
                Fragment::Text(text) => self.text(text),
                Fragment::Newline => self.newline(),
                Fragment::Indented(inner) => self.indented(inner),
                Fragment::Spanned(span, inner) => self.spanned(*span, inner),
            }
        }
    }

    pub fn finish(self) -> (String, SpanMap) {
        (self.output, self.span_map)
    }

    fn text(&mut self, text: &str) {
        if self.at_line_start {
            self.output.push_str(&Self::INDENT.repeat(self.indentation));
            self.at_line_start = false;
        }
        self.output.push_str(text);
    }

    fn newline(&mut self) {
        self.output.push('\n');
        self.at_line_start = true;
    }

    fn indented(&mut self, inner: &[Fragment]) {
        self.indentation += 1;
        self.write(inner);
        self.indentation -= 1;
    }

    fn spanned(&mut self, span: Span, inner: &[Fragment]) {
        let start = self.output.len();
        self.write(inner);
        self.span_map.entries.push((start..self.output.len(), span));
    }
}
