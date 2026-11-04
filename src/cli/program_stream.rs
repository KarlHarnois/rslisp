use std::collections::VecDeque;

#[derive(Debug, PartialEq, Eq)]
pub enum StreamedText {
    Output(String),
    Value { text: String, form: usize },
}

pub struct ProgramStream {
    marker: Vec<u8>,
    forms: VecDeque<usize>,
    pending: Vec<u8>,
    value_of_form: Option<usize>,
}

impl ProgramStream {
    const NEWLINE: u8 = b'\n';

    pub fn new(marker: &str, forms: Vec<usize>) -> Self {
        Self {
            marker: marker.as_bytes().to_vec(),
            forms: forms.into(),
            pending: Vec::new(),
            value_of_form: None,
        }
    }

    pub fn unmarked() -> Self {
        Self::new("", Vec::new())
    }

    pub fn read(&mut self, bytes: &[u8]) -> Vec<StreamedText> {
        self.pending.extend_from_slice(bytes);
        let mut texts = Vec::new();
        while let Some(text) = self.next_text() {
            texts.push(text);
        }
        texts.extend(self.released_output());
        texts.retain(|text| *text != StreamedText::Output(String::new()));
        texts
    }

    pub fn finish(mut self) -> Option<StreamedText> {
        let rest = Self::text(&std::mem::take(&mut self.pending));
        match self.value_of_form {
            Some(form) => Some(StreamedText::Value { text: rest, form }),
            None => (!rest.is_empty()).then_some(StreamedText::Output(rest)),
        }
    }

    fn next_text(&mut self) -> Option<StreamedText> {
        match self.value_of_form {
            Some(form) => self.next_value(form),
            None => self.output_before_marker(),
        }
    }

    fn next_value(&mut self, form: usize) -> Option<StreamedText> {
        let end = self
            .pending
            .iter()
            .position(|byte| *byte == Self::NEWLINE)?;
        let line: Vec<u8> = self.pending.drain(..=end).collect();
        self.value_of_form = None;
        Some(StreamedText::Value {
            text: Self::text(&line[..end]),
            form,
        })
    }

    fn output_before_marker(&mut self) -> Option<StreamedText> {
        let start = self.marker_start()?;
        let output: Vec<u8> = self.pending.drain(..start).collect();
        self.pending.drain(..self.marker.len());
        self.value_of_form = self.forms.pop_front();
        Some(StreamedText::Output(Self::text(&output)))
    }

    fn marker_start(&self) -> Option<usize> {
        if self.forms.is_empty() {
            return None;
        }
        self.pending
            .windows(self.marker.len())
            .position(|window| window == self.marker)
    }

    fn released_output(&mut self) -> Option<StreamedText> {
        if self.value_of_form.is_some() {
            return None;
        }
        let released = Self::complete_characters(&self.pending[..self.held_back_start()]);
        let output: Vec<u8> = self.pending.drain(..released).collect();
        Some(StreamedText::Output(Self::text(&output)))
    }

    fn held_back_start(&self) -> usize {
        let length = self.pending.len();
        if self.forms.is_empty() {
            return length;
        }
        (1..self.marker.len().min(length + 1))
            .rev()
            .find(|held| self.pending.ends_with(&self.marker[..*held]))
            .map_or(length, |held| length - held)
    }

    fn complete_characters(bytes: &[u8]) -> usize {
        let mut start = 0;
        loop {
            match std::str::from_utf8(&bytes[start..]) {
                Ok(_) => return bytes.len(),
                Err(error) => match error.error_len() {
                    Some(invalid) => start += error.valid_up_to() + invalid,
                    None => return start + error.valid_up_to(),
                },
            }
        }
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{ProgramStream, StreamedText};

    struct StreamFixture {
        stream: ProgramStream,
        texts: Vec<StreamedText>,
    }

    impl StreamFixture {
        const MARKER: &str = "MARK";

        fn marking(forms: Vec<usize>) -> Self {
            Self {
                stream: ProgramStream::new(Self::MARKER, forms),
                texts: Vec::new(),
            }
        }

        fn unmarked() -> Self {
            Self {
                stream: ProgramStream::unmarked(),
                texts: Vec::new(),
            }
        }

        fn read(mut self, bytes: &[u8]) -> Self {
            self.texts.extend(self.stream.read(bytes));
            self
        }

        fn read_texts(&self) -> &[StreamedText] {
            &self.texts
        }

        fn finished(mut self) -> Vec<StreamedText> {
            self.texts.extend(self.stream.finish());
            self.texts
        }

        fn output(text: &str) -> StreamedText {
            StreamedText::Output(text.to_string())
        }

        fn value(text: &str, form: usize) -> StreamedText {
            StreamedText::Value {
                text: text.to_string(),
                form,
            }
        }
    }

    #[test]
    fn output_before_a_marker_comes_before_the_value_of_the_form() {
        let texts = StreamFixture::marking(vec![2]).read(b"xMARK6\n").finished();
        assert_eq!(
            texts,
            [StreamFixture::output("x"), StreamFixture::value("6", 2)]
        );
    }

    #[test]
    fn a_marker_split_across_two_reads_is_still_found() {
        let fixture = StreamFixture::marking(vec![1]).read(b"hiMA");
        assert_eq!(fixture.read_texts(), [StreamFixture::output("hi")]);
        let texts = fixture.read(b"RK7\n").finished();
        assert_eq!(
            texts,
            [StreamFixture::output("hi"), StreamFixture::value("7", 1)]
        );
    }

    #[test]
    fn output_that_cannot_start_a_marker_is_not_held_back() {
        let fixture = StreamFixture::marking(vec![1]).read(b"name? ");
        assert_eq!(fixture.read_texts(), [StreamFixture::output("name? ")]);
    }

    #[test]
    fn a_value_split_across_two_reads_waits_for_its_newline() {
        let fixture = StreamFixture::marking(vec![1]).read(b"MARK(some");
        assert!(fixture.read_texts().is_empty());
        let texts = fixture.read(b" 1)\nafter\n").finished();
        assert_eq!(
            texts,
            [
                StreamFixture::value("(some 1)", 1),
                StreamFixture::output("after\n"),
            ]
        );
    }

    #[test]
    fn each_marker_takes_the_next_printed_form() {
        let texts = StreamFixture::marking(vec![1, 3])
            .read(b"MARK1\nMARK3\n")
            .finished();
        assert_eq!(
            texts,
            [StreamFixture::value("1", 1), StreamFixture::value("3", 3)]
        );
    }

    #[test]
    fn a_marker_after_the_last_printed_form_is_output() {
        let texts = StreamFixture::marking(Vec::new())
            .read(b"MARK1\n")
            .finished();
        assert_eq!(texts, [StreamFixture::output("MARK1\n")]);
    }

    #[test]
    fn a_value_cut_short_by_the_end_of_the_stream_is_still_a_value() {
        let texts = StreamFixture::marking(vec![1]).read(b"MARK5").finished();
        assert_eq!(texts, [StreamFixture::value("5", 1)]);
    }

    #[test]
    fn output_after_the_last_printed_form_is_not_held_back() {
        let fixture = StreamFixture::marking(Vec::new()).read(b"oMA");
        assert_eq!(fixture.read_texts(), [StreamFixture::output("oMA")]);
    }

    #[test]
    fn a_held_back_prefix_of_the_marker_is_output_at_the_end() {
        let texts = StreamFixture::marking(vec![1]).read(b"oMA").finished();
        assert_eq!(
            texts,
            [StreamFixture::output("o"), StreamFixture::output("MA")]
        );
    }

    #[test]
    fn a_character_split_across_two_reads_is_output_whole() {
        let snowman = "☃".as_bytes();
        let fixture = StreamFixture::unmarked().read(&snowman[..1]);
        assert!(fixture.read_texts().is_empty());
        let texts = fixture.read(&snowman[1..]).finished();
        assert_eq!(texts, [StreamFixture::output("☃")]);
    }

    #[test]
    fn an_invalid_byte_is_output_as_a_replacement_character() {
        let texts = StreamFixture::unmarked().read(b"a\xffb").finished();
        assert_eq!(texts, [StreamFixture::output("a\u{fffd}b")]);
    }

    #[test]
    fn an_empty_stream_finishes_with_no_text() {
        assert!(StreamFixture::unmarked().finished().is_empty());
    }
}
