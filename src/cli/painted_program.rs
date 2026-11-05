use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use super::finished_program::FinishedProgram;
use super::program_stream::{ProgramStream, Streamed, StreamedBytes};
use super::token_colors::TokenColors;

pub struct PaintedProgram {
    stream: ProgramStream,
}

impl PaintedProgram {
    const READ_SIZE: usize = 8192;

    pub fn new(stream: ProgramStream) -> Self {
        Self { stream }
    }

    pub fn run(self, binary: &Path, terminal: impl Write) -> FinishedProgram {
        let status = Command::new(binary)
            .stdout(Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                let output = child.stdout.take().expect("standard output is piped");
                self.forward(output, terminal);
                child.wait()
            });
        FinishedProgram::of(binary, status)
    }

    fn forward(self, mut source: impl Read, mut terminal: impl Write) {
        let Self { mut stream } = self;
        let mut buffer = [0; Self::READ_SIZE];
        while let Ok(read @ 1..) = source.read(&mut buffer) {
            if Self::write_all(&mut terminal, stream.read(&buffer[..read])).is_err() {
                return;
            }
        }
        let _ = Self::write_all(&mut terminal, stream.finish());
    }

    fn write_all(
        terminal: &mut impl Write,
        parts: impl IntoIterator<Item = StreamedBytes>,
    ) -> io::Result<()> {
        for part in parts {
            Self::write(terminal, part)?;
        }
        terminal.flush()
    }

    fn write(terminal: &mut impl Write, part: StreamedBytes) -> io::Result<()> {
        match part {
            Streamed::Output(bytes) => terminal.write_all(&bytes),
            Streamed::Value { text, .. } => {
                writeln!(terminal, "{}", TokenColors::Painted.paint(&text))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor, Write};

    use super::PaintedProgram;
    use crate::cli::program_stream::ProgramStream;
    use crate::cli::token_colors::TokenColors;

    struct PaintFixture;

    struct ClosedTerminal;

    impl Write for ClosedTerminal {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl PaintFixture {
        const MARKER: &str = "MARK";

        fn written(reads: &[&[u8]]) -> Vec<u8> {
            let mut terminal = Vec::new();
            let program = PaintedProgram::new(ProgramStream::new(Self::MARKER, vec![1]));
            program.forward(reads.concat().as_slice(), &mut terminal);
            terminal
        }
    }

    #[test]
    fn the_program_bytes_reach_the_terminal_unchanged() {
        let written = PaintFixture::written(&[b"\x1b[31mred\x1b[0m \xff\n"]);
        assert_eq!(written, b"\x1b[31mred\x1b[0m \xff\n");
    }

    #[test]
    fn a_value_after_the_marker_takes_the_token_colors() {
        let written = PaintFixture::written(&[b"out\nMARK(some \"a\")\n"]);
        let painted = TokenColors::Painted.paint("(some \"a\")");
        assert_eq!(written, format!("out\n{painted}\n").into_bytes());
    }

    #[test]
    fn a_closed_terminal_stops_the_reading() {
        let mut source = Cursor::new(vec![b'x'; PaintedProgram::READ_SIZE * 2]);
        let program = PaintedProgram::new(ProgramStream::unmarked());
        program.forward(&mut source, ClosedTerminal);
        assert_eq!(source.position(), PaintedProgram::READ_SIZE as u64);
    }
}
