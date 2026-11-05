use super::program_stream::{Streamed, StreamedBytes};

pub type StreamedText = Streamed<String>;

#[derive(Default)]
pub struct WholeCharacters {
    pending: Vec<u8>,
}

impl WholeCharacters {
    pub fn texts(&mut self, parts: impl IntoIterator<Item = StreamedBytes>) -> Vec<StreamedText> {
        parts
            .into_iter()
            .flat_map(|part| self.texts_of(part))
            .collect()
    }

    pub fn rest(&mut self) -> Option<StreamedText> {
        let rest = std::mem::take(&mut self.pending);
        Self::output(&rest)
    }

    fn texts_of(&mut self, part: StreamedBytes) -> Vec<StreamedText> {
        match part {
            Streamed::Output(bytes) => self.released(&bytes).into_iter().collect(),
            Streamed::Value { text, form } => {
                let value = Streamed::Value { text, form };
                self.rest().into_iter().chain([value]).collect()
            }
        }
    }

    fn released(&mut self, bytes: &[u8]) -> Option<StreamedText> {
        self.pending.extend_from_slice(bytes);
        let complete = Self::complete_characters(&self.pending);
        let released: Vec<u8> = self.pending.drain(..complete).collect();
        Self::output(&released)
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

    fn output(bytes: &[u8]) -> Option<StreamedText> {
        (!bytes.is_empty()).then(|| Streamed::Output(String::from_utf8_lossy(bytes).into_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::{Streamed, StreamedText, WholeCharacters};

    struct CharactersFixture;

    impl CharactersFixture {
        fn output(text: &str) -> StreamedText {
            Streamed::Output(text.to_string())
        }

        fn output_bytes(bytes: &[u8]) -> Streamed<Vec<u8>> {
            Streamed::Output(bytes.to_vec())
        }
    }

    #[test]
    fn a_character_split_across_two_reads_is_output_whole() {
        let snowman = "☃".as_bytes();
        let mut characters = WholeCharacters::default();
        assert!(
            characters
                .texts([CharactersFixture::output_bytes(&snowman[..1])])
                .is_empty()
        );
        assert_eq!(
            characters.texts([CharactersFixture::output_bytes(&snowman[1..])]),
            [CharactersFixture::output("☃")]
        );
    }

    #[test]
    fn an_invalid_byte_is_output_as_a_replacement_character() {
        let mut characters = WholeCharacters::default();
        assert_eq!(
            characters.texts([CharactersFixture::output_bytes(b"a\xffb")]),
            [CharactersFixture::output("a\u{fffd}b")]
        );
    }

    #[test]
    fn part_of_a_character_before_a_value_is_output_first() {
        let mut characters = WholeCharacters::default();
        let value = Streamed::Value {
            text: "1".to_string(),
            form: 1,
        };
        assert_eq!(
            characters.texts([CharactersFixture::output_bytes(b"\xe2"), value]),
            [
                CharactersFixture::output("\u{fffd}"),
                Streamed::Value {
                    text: "1".to_string(),
                    form: 1
                }
            ]
        );
    }

    #[test]
    fn part_of_a_character_at_the_end_is_the_rest() {
        let mut characters = WholeCharacters::default();
        characters.texts([CharactersFixture::output_bytes(b"\xe2")]);
        assert_eq!(
            characters.rest(),
            Some(CharactersFixture::output("\u{fffd}"))
        );
        assert_eq!(characters.rest(), None);
    }
}
