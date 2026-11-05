use std::collections::VecDeque;

#[derive(Debug, PartialEq, Eq)]
pub enum Streamed<T> {
    Output(T),
    Value { text: String, form: usize },
}

pub type StreamedBytes = Streamed<Vec<u8>>;

pub struct ProgramStream {
    marker: Vec<u8>,
    values: ExpectedValues,
    pending: Vec<u8>,
    value_of_form: Option<usize>,
}

enum ExpectedValues {
    Forms(VecDeque<usize>),
    Records(usize),
}

impl ExpectedValues {
    fn is_exhausted(&self) -> bool {
        match self {
            Self::Forms(forms) => forms.is_empty(),
            Self::Records(_) => false,
        }
    }

    fn next(&mut self) -> Option<usize> {
        match self {
            Self::Forms(forms) => forms.pop_front(),
            Self::Records(read) => {
                *read += 1;
                Some(*read)
            }
        }
    }
}

impl ProgramStream {
    const NEWLINE: u8 = b'\n';

    pub fn new(marker: &str, forms: Vec<usize>) -> Self {
        Self::expecting(marker, ExpectedValues::Forms(forms.into()))
    }

    pub fn recording(marker: &str) -> Self {
        Self::expecting(marker, ExpectedValues::Records(0))
    }

    fn expecting(marker: &str, values: ExpectedValues) -> Self {
        Self {
            marker: marker.as_bytes().to_vec(),
            values,
            pending: Vec::new(),
            value_of_form: None,
        }
    }

    pub fn unmarked() -> Self {
        Self::new("", Vec::new())
    }

    pub fn read(&mut self, bytes: &[u8]) -> Vec<StreamedBytes> {
        self.pending.extend_from_slice(bytes);
        let mut parts = Vec::new();
        while let Some(part) = self.next_part() {
            parts.push(part);
        }
        parts.extend(self.released_output());
        parts.retain(|part| *part != Streamed::Output(Vec::new()));
        parts
    }

    pub fn finish(mut self) -> Option<StreamedBytes> {
        let rest = std::mem::take(&mut self.pending);
        match self.value_of_form {
            Some(form) => Some(Streamed::Value {
                text: Self::text(&rest),
                form,
            }),
            None => (!rest.is_empty()).then_some(Streamed::Output(rest)),
        }
    }

    fn next_part(&mut self) -> Option<StreamedBytes> {
        match self.value_of_form {
            Some(form) => self.next_value(form),
            None => self.output_before_marker(),
        }
    }

    fn next_value(&mut self, form: usize) -> Option<StreamedBytes> {
        let end = self
            .pending
            .iter()
            .position(|byte| *byte == Self::NEWLINE)?;
        let line: Vec<u8> = self.pending.drain(..=end).collect();
        self.value_of_form = None;
        Some(Streamed::Value {
            text: Self::text(&line[..end]),
            form,
        })
    }

    fn output_before_marker(&mut self) -> Option<StreamedBytes> {
        let start = self.marker_start()?;
        let output: Vec<u8> = self.pending.drain(..start).collect();
        self.pending.drain(..self.marker.len());
        self.value_of_form = self.values.next();
        Some(Streamed::Output(output))
    }

    fn marker_start(&self) -> Option<usize> {
        if self.values.is_exhausted() {
            return None;
        }
        self.pending
            .windows(self.marker.len())
            .position(|window| window == self.marker)
    }

    fn released_output(&mut self) -> Option<StreamedBytes> {
        if self.value_of_form.is_some() {
            return None;
        }
        let released = self.held_back_start();
        Some(Streamed::Output(self.pending.drain(..released).collect()))
    }

    fn held_back_start(&self) -> usize {
        let length = self.pending.len();
        if self.values.is_exhausted() {
            return length;
        }
        (1..self.marker.len().min(length + 1))
            .rev()
            .find(|held| self.pending.ends_with(&self.marker[..*held]))
            .map_or(length, |held| length - held)
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{ProgramStream, Streamed, StreamedBytes};

    struct StreamFixture {
        stream: ProgramStream,
        parts: Vec<StreamedBytes>,
    }

    impl StreamFixture {
        const MARKER: &str = "MARK";

        fn marking(forms: Vec<usize>) -> Self {
            Self {
                stream: ProgramStream::new(Self::MARKER, forms),
                parts: Vec::new(),
            }
        }

        fn unmarked() -> Self {
            Self {
                stream: ProgramStream::unmarked(),
                parts: Vec::new(),
            }
        }

        fn recording() -> Self {
            Self {
                stream: ProgramStream::recording(Self::MARKER),
                parts: Vec::new(),
            }
        }

        fn read(mut self, bytes: &[u8]) -> Self {
            self.parts.extend(self.stream.read(bytes));
            self
        }

        fn read_parts(&self) -> &[StreamedBytes] {
            &self.parts
        }

        fn finished(mut self) -> Vec<StreamedBytes> {
            self.parts.extend(self.stream.finish());
            self.parts
        }

        fn output(bytes: &[u8]) -> StreamedBytes {
            Streamed::Output(bytes.to_vec())
        }

        fn value(text: &str, form: usize) -> StreamedBytes {
            Streamed::Value {
                text: text.to_string(),
                form,
            }
        }
    }

    #[test]
    fn output_before_a_marker_comes_before_the_value_of_the_form() {
        let parts = StreamFixture::marking(vec![2]).read(b"xMARK6\n").finished();
        assert_eq!(
            parts,
            [StreamFixture::output(b"x"), StreamFixture::value("6", 2)]
        );
    }

    #[test]
    fn a_marker_split_across_two_reads_is_still_found() {
        let fixture = StreamFixture::marking(vec![1]).read(b"hiMA");
        assert_eq!(fixture.read_parts(), [StreamFixture::output(b"hi")]);
        let parts = fixture.read(b"RK7\n").finished();
        assert_eq!(
            parts,
            [StreamFixture::output(b"hi"), StreamFixture::value("7", 1)]
        );
    }

    #[test]
    fn output_that_cannot_start_a_marker_is_not_held_back() {
        let fixture = StreamFixture::marking(vec![1]).read(b"name? ");
        assert_eq!(fixture.read_parts(), [StreamFixture::output(b"name? ")]);
    }

    #[test]
    fn a_value_split_across_two_reads_waits_for_its_newline() {
        let fixture = StreamFixture::marking(vec![1]).read(b"MARK(some");
        assert!(fixture.read_parts().is_empty());
        let parts = fixture.read(b" 1)\nafter\n").finished();
        assert_eq!(
            parts,
            [
                StreamFixture::value("(some 1)", 1),
                StreamFixture::output(b"after\n"),
            ]
        );
    }

    #[test]
    fn each_marker_takes_the_next_printed_form() {
        let parts = StreamFixture::marking(vec![1, 3])
            .read(b"MARK1\nMARK3\n")
            .finished();
        assert_eq!(
            parts,
            [StreamFixture::value("1", 1), StreamFixture::value("3", 3)]
        );
    }

    #[test]
    fn a_marker_after_the_last_printed_form_is_output() {
        let parts = StreamFixture::marking(Vec::new())
            .read(b"MARK1\n")
            .finished();
        assert_eq!(parts, [StreamFixture::output(b"MARK1\n")]);
    }

    #[test]
    fn a_value_cut_short_by_the_end_of_the_stream_is_still_a_value() {
        let parts = StreamFixture::marking(vec![1]).read(b"MARK5").finished();
        assert_eq!(parts, [StreamFixture::value("5", 1)]);
    }

    #[test]
    fn output_after_the_last_printed_form_is_not_held_back() {
        let fixture = StreamFixture::marking(Vec::new()).read(b"oMA");
        assert_eq!(fixture.read_parts(), [StreamFixture::output(b"oMA")]);
    }

    #[test]
    fn a_held_back_prefix_of_the_marker_is_output_at_the_end() {
        let parts = StreamFixture::marking(vec![1]).read(b"oMA").finished();
        assert_eq!(
            parts,
            [StreamFixture::output(b"o"), StreamFixture::output(b"MA")]
        );
    }

    #[test]
    fn an_invalid_byte_is_output_unchanged() {
        let parts = StreamFixture::unmarked().read(b"a\xffb").finished();
        assert_eq!(parts, [StreamFixture::output(b"a\xffb")]);
    }

    #[test]
    fn part_of_a_character_is_not_held_back() {
        let snowman = "☃".as_bytes();
        let fixture = StreamFixture::unmarked().read(&snowman[..1]);
        assert_eq!(fixture.read_parts(), [StreamFixture::output(&snowman[..1])]);
    }

    #[test]
    fn a_recording_stream_reads_every_marked_record_in_order() {
        let parts = StreamFixture::recording()
            .read(b"MARK(start 2)\nhiMARK(ok)\nMARK(done)\n")
            .finished();
        assert_eq!(
            parts,
            [
                StreamFixture::value("(start 2)", 1),
                StreamFixture::output(b"hi"),
                StreamFixture::value("(ok)", 2),
                StreamFixture::value("(done)", 3),
            ]
        );
    }

    #[test]
    fn a_recording_stream_holds_back_a_possible_marker() {
        let fixture = StreamFixture::recording().read(b"oMA");
        assert_eq!(fixture.read_parts(), [StreamFixture::output(b"o")]);
    }

    #[test]
    fn an_empty_stream_finishes_with_no_text() {
        assert!(StreamFixture::unmarked().finished().is_empty());
    }
}
