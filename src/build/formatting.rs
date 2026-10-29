use std::fmt;
use std::io::{self, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use super::token_alignment::TokenAlignment;
use crate::emit::EmittedCrate;
use crate::language::Language;

pub struct EmittedFormatting {
    rustfmt: Rustfmt,
}

impl EmittedFormatting {
    pub const TIME_LIMIT: Duration = Duration::from_secs(10);

    pub fn apply(emitted: &EmittedCrate) -> Result<EmittedCrate, FormattingError> {
        Self::within(Self::TIME_LIMIT).format(emitted)
    }

    pub fn within(time_limit: Duration) -> Self {
        Self {
            rustfmt: Rustfmt::new(time_limit),
        }
    }

    pub fn time_limit_variable() -> String {
        format!(
            "{}_RUSTFMT_TIME_LIMIT_MILLISECONDS",
            Language::NAME.to_uppercase()
        )
    }

    pub fn format(&self, emitted: &EmittedCrate) -> Result<EmittedCrate, FormattingError> {
        let code = self.rustfmt.format(&emitted.code)?;
        let alignment = TokenAlignment::new(&emitted.code, &code);
        let span_map = alignment
            .remap(&emitted.span_map)
            .ok_or(FormattingError::Misaligned)?;
        Ok(EmittedCrate {
            code,
            span_map,
            names: emitted.names.clone(),
        })
    }
}

#[derive(Debug)]
pub enum FormattingError {
    Unavailable(io::Error),
    Failed(String),
    TimedOut(Duration),
    Misaligned,
}

impl fmt::Display for FormattingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(error) => write!(formatter, "cannot run rustfmt: {error}"),
            Self::Failed(message) if message.is_empty() => write!(formatter, "rustfmt failed"),
            Self::Failed(message) => write!(formatter, "rustfmt failed: {message}"),
            Self::TimedOut(limit) => write!(formatter, "rustfmt did not finish within {limit:?}"),
            Self::Misaligned => {
                write!(
                    formatter,
                    "cannot align rustfmt's output with the emitted Rust"
                )
            }
        }
    }
}

struct Rustfmt {
    time_limit: Duration,
}

impl Rustfmt {
    const PROGRAM: &'static str = "rustfmt";
    const ARGUMENTS: [&'static str; 2] = ["--edition", "2024"];

    fn new(time_limit: Duration) -> Self {
        Self { time_limit }
    }

    fn format(&self, source: &str) -> Result<String, FormattingError> {
        let mut child = Self::spawn()?;
        let output = PipeReader::start(child.stdout.take().expect("standard output is piped"));
        let errors = PipeReader::start(child.stderr.take().expect("standard error is piped"));
        Self::send(&mut child, source)?;
        let formatted = self.read_within_limit(&mut child, output)?;
        let status = child.wait().map_err(FormattingError::Unavailable)?;
        if !status.success() {
            return Err(FormattingError::Failed(errors.read_message()));
        }
        String::from_utf8(formatted)
            .map_err(|_| FormattingError::Failed("rustfmt wrote non-UTF-8 output".to_string()))
    }

    fn spawn() -> Result<Child, FormattingError> {
        Command::new(Self::PROGRAM)
            .args(Self::ARGUMENTS)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(FormattingError::Unavailable)
    }

    fn send(child: &mut Child, source: &str) -> Result<(), FormattingError> {
        let mut input = child.stdin.take().expect("standard input is piped");
        input
            .write_all(source.as_bytes())
            .map_err(|error| FormattingError::Failed(error.to_string()))
    }

    fn read_within_limit(
        &self,
        child: &mut Child,
        output: PipeReader,
    ) -> Result<Vec<u8>, FormattingError> {
        match output.read_within(self.time_limit) {
            Some(bytes) => bytes.map_err(|error| FormattingError::Failed(error.to_string())),
            None => {
                Self::stop(child);
                Err(FormattingError::TimedOut(self.time_limit))
            }
        }
    }

    fn stop(child: &mut Child) {
        child.kill().ok();
        child.wait().ok();
    }
}

struct PipeReader {
    contents: Receiver<io::Result<Vec<u8>>>,
}

impl PipeReader {
    fn start(mut pipe: impl Read + Send + 'static) -> Self {
        let (sender, contents) = mpsc::channel();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let read = pipe.read_to_end(&mut bytes).map(|_| bytes);
            sender.send(read).ok();
        });
        Self { contents }
    }

    fn read_within(&self, time_limit: Duration) -> Option<io::Result<Vec<u8>>> {
        self.contents.recv_timeout(time_limit).ok()
    }

    fn read_message(&self) -> String {
        let bytes = self.contents.recv().ok().and_then(Result::ok);
        String::from_utf8_lossy(&bytes.unwrap_or_default())
            .trim()
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_each_formatting_failure() {
        let unavailable = FormattingError::Unavailable(io::Error::from(io::ErrorKind::NotFound));
        assert!(unavailable.to_string().contains("rustfmt"));
        let failed = FormattingError::Failed("boom".to_string());
        assert!(failed.to_string().contains("boom"));
        let empty = FormattingError::Failed(String::new());
        assert_eq!(empty.to_string(), "rustfmt failed");
        let misaligned = FormattingError::Misaligned;
        assert!(misaligned.to_string().contains("align"));
    }

    #[test]
    fn names_the_time_limit_that_rustfmt_exceeded() {
        let timed_out = FormattingError::TimedOut(EmittedFormatting::TIME_LIMIT);
        assert_eq!(timed_out.to_string(), "rustfmt did not finish within 10s");
    }
}
