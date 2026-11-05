use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;

use super::finished_program::FinishedProgram;
use super::json_replies::{JsonObject, JsonReplies};
use super::program_stream::ProgramStream;
use super::program_stream::Streamed;
use super::whole_characters::{StreamedText, WholeCharacters};

pub struct StreamedProgram {
    replies: Arc<JsonReplies>,
}

struct OutputTexts {
    stream: ProgramStream,
    characters: WholeCharacters,
}

struct StreamForwarder {
    replies: Arc<JsonReplies>,
    output_field: &'static str,
}

impl StreamedProgram {
    const OUTPUT: &'static str = "out";
    const ERRORS: &'static str = "err";

    pub fn new(replies: Arc<JsonReplies>) -> Self {
        Self { replies }
    }

    pub fn run(&self, binary: &Path, output: ProgramStream) -> FinishedProgram {
        let spawned = Command::new(binary)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(error) => return FinishedProgram::of(binary, Err(error)),
        };
        let errors = child.stderr.take().expect("standard error is piped");
        let error_forwarder = self.forwarder(Self::ERRORS);
        let error_thread =
            thread::spawn(move || error_forwarder.forward(errors, ProgramStream::unmarked()));
        let standard_output = child.stdout.take().expect("standard output is piped");
        self.forwarder(Self::OUTPUT)
            .forward(standard_output, output);
        error_thread
            .join()
            .expect("forwarding standard error does not panic");
        FinishedProgram::of(binary, child.wait())
    }

    fn forwarder(&self, output_field: &'static str) -> StreamForwarder {
        StreamForwarder {
            replies: Arc::clone(&self.replies),
            output_field,
        }
    }
}

impl StreamForwarder {
    const VALUE: &'static str = "value";
    const FORM: &'static str = "form";

    fn forward(&self, source: impl Read, stream: ProgramStream) {
        OutputTexts::new(stream).send_each(source, |text| self.replies.send(self.reply(text)));
    }

    fn reply(&self, text: StreamedText) -> JsonObject {
        match text {
            Streamed::Output(text) => JsonObject::new(self.output_field, text),
            Streamed::Value { text, form } => {
                JsonObject::new(Self::VALUE, text).with(Self::FORM, form)
            }
        }
    }
}

impl OutputTexts {
    const READ_SIZE: usize = 8192;

    fn new(stream: ProgramStream) -> Self {
        Self {
            stream,
            characters: WholeCharacters::default(),
        }
    }

    fn send_each(
        self,
        mut source: impl Read,
        mut send: impl FnMut(StreamedText) -> io::Result<()>,
    ) {
        let Self {
            mut stream,
            mut characters,
        } = self;
        let mut buffer = [0; Self::READ_SIZE];
        while let Ok(read @ 1..) = source.read(&mut buffer) {
            let texts = characters.texts(stream.read(&buffer[..read]));
            if texts.into_iter().try_for_each(&mut send).is_err() {
                return;
            }
        }
        let finished = characters.texts(stream.finish());
        let _ = finished
            .into_iter()
            .chain(characters.rest())
            .try_for_each(send);
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::OutputTexts;
    use crate::cli::program_stream::{ProgramStream, Streamed};
    use crate::cli::whole_characters::StreamedText;

    struct TextsFixture;

    impl TextsFixture {
        fn sent(source: &[u8]) -> Vec<StreamedText> {
            let mut sent = Vec::new();
            OutputTexts::new(ProgramStream::unmarked()).send_each(source, |text| {
                sent.push(text);
                Ok(())
            });
            sent
        }

        fn output(text: &str) -> StreamedText {
            Streamed::Output(text.to_string())
        }
    }

    #[test]
    fn part_of_a_character_at_the_end_of_the_output_is_sent_as_a_replacement() {
        assert_eq!(
            TextsFixture::sent(b"a\xe2"),
            [TextsFixture::output("a"), TextsFixture::output("\u{fffd}")]
        );
    }

    #[test]
    fn a_failed_send_stops_the_reading() {
        let mut source = io::Cursor::new(vec![b'x'; OutputTexts::READ_SIZE * 2]);
        OutputTexts::new(ProgramStream::unmarked())
            .send_each(&mut source, |_| Err(io::ErrorKind::BrokenPipe.into()));
        assert_eq!(source.position(), OutputTexts::READ_SIZE as u64);
    }
}
