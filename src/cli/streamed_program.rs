use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;

use super::finished_program::FinishedProgram;
use super::json_replies::{JsonObject, JsonReplies};
use super::program_stream::{ProgramStream, StreamedText};

pub struct StreamedProgram {
    replies: Arc<JsonReplies>,
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
    const READ_SIZE: usize = 8192;

    fn forward(&self, mut source: impl Read, mut stream: ProgramStream) {
        let mut buffer = [0; Self::READ_SIZE];
        while let Ok(read @ 1..) = source.read(&mut buffer) {
            if self.send_all(stream.read(&buffer[..read])).is_err() {
                return;
            }
        }
        let _ = self.send_all(stream.finish());
    }

    fn send_all(&self, texts: impl IntoIterator<Item = StreamedText>) -> io::Result<()> {
        texts
            .into_iter()
            .try_for_each(|text| self.replies.send(self.reply(text)))
    }

    fn reply(&self, text: StreamedText) -> JsonObject {
        match text {
            StreamedText::Output(text) => JsonObject::new(self.output_field, text),
            StreamedText::Value { text, form } => {
                JsonObject::new(Self::VALUE, text).with(Self::FORM, form)
            }
        }
    }
}
