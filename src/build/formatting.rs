use std::fmt;
use std::io::{self, Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::interrupt_signal::InterruptSignal;
use super::token_alignment::TokenAlignment;
use crate::emit::{EmittedCrate, FormattingWork};
use crate::language::Language;

pub struct EmittedFormatting {
    rustfmt: Rustfmt,
}

impl EmittedFormatting {
    pub const TIME_LIMIT: Duration = Duration::from_secs(10);

    pub fn within(time_limit: Duration) -> Self {
        Self {
            rustfmt: Rustfmt::new(time_limit),
        }
    }

    #[inline(never)]
    pub fn apply(&self, emitted: &EmittedCrate) -> Result<EmittedCrate, FormattingError> {
        let code = self.rustfmt.format(&emitted.code)?;
        let alignment = TokenAlignment::new(&emitted.code, &code);
        let span_map = alignment
            .remap(&emitted.span_map)
            .ok_or(FormattingError::Misaligned)?;
        Ok(EmittedCrate {
            code,
            span_map,
            names: emitted.names.clone(),
            emission: emitted.emission,
            formatting: Some(FormattingWork {
                scanned_tokens: alignment.scanned_tokens(),
            }),
        })
    }

    pub fn time_limit_variable() -> String {
        format!(
            "{}_RUSTFMT_TIME_LIMIT_MILLISECONDS",
            Language::NAME.to_uppercase()
        )
    }
}

#[derive(Debug)]
pub enum FormattingError {
    Unavailable(io::Error),
    Failed(String),
    TimedOut(Duration),
    Misaligned,
    Interrupted,
}

impl fmt::Display for FormattingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(error) => write!(formatter, "cannot run rustfmt: {error}"),
            Self::Failed(message) if message.is_empty() => write!(formatter, "rustfmt failed"),
            Self::Failed(message) => write!(formatter, "rustfmt failed: {message}"),
            Self::TimedOut(limit) => write!(formatter, "rustfmt did not finish within {limit:?}"),
            Self::Interrupted => write!(formatter, "rustfmt was interrupted"),
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
        let formatted = self.communicate(&mut child, source, &output, &errors);
        output.join();
        errors.join();
        formatted
    }

    fn communicate(
        &self,
        child: &mut Child,
        source: &str,
        output: &PipeReader,
        errors: &PipeReader,
    ) -> Result<String, FormattingError> {
        Self::send(child, source).map_err(|error| Self::explain_failed_send(child, error))?;
        let formatted = self
            .read_within_limit(output)
            .inspect_err(|_| _ = Self::stop(child))?;
        let status = child.wait().map_err(FormattingError::Unavailable)?;
        if InterruptSignal::stopped(status) {
            return Err(FormattingError::Interrupted);
        }
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

    fn explain_failed_send(child: &mut Child, error: FormattingError) -> FormattingError {
        let interrupted = Self::stop(child).is_some_and(InterruptSignal::stopped);
        if interrupted {
            FormattingError::Interrupted
        } else {
            error
        }
    }

    fn read_within_limit(&self, output: &PipeReader) -> Result<Vec<u8>, FormattingError> {
        output
            .read_within(self.time_limit)
            .ok_or(FormattingError::TimedOut(self.time_limit))?
            .map_err(|error| FormattingError::Failed(error.to_string()))
    }

    fn stop(child: &mut Child) -> Option<ExitStatus> {
        child.kill().ok();
        child.wait().ok()
    }
}

struct PipeReader {
    contents: Receiver<io::Result<Vec<u8>>>,
    thread: Option<JoinHandle<()>>,
}

impl PipeReader {
    fn start(mut pipe: impl Read + Send + 'static) -> Self {
        let (sender, contents) = mpsc::channel();
        let thread = thread::spawn(move || {
            let mut bytes = Vec::new();
            let read = pipe.read_to_end(&mut bytes).map(|_| bytes);
            sender.send(read).ok();
        });
        Self {
            contents,
            thread: Some(thread),
        }
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

    fn join(mut self) {
        let thread = self.thread.take().expect("only join takes the thread");
        thread.join().ok();
    }
}

impl Drop for PipeReader {
    fn drop(&mut self) {
        debug_assert!(
            self.thread.is_none(),
            "each pipe reader is joined before it drops"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    struct EmptyPipe {
        dropped: Arc<AtomicBool>,
    }

    impl EmptyPipe {
        fn new(dropped: &Arc<AtomicBool>) -> Self {
            Self {
                dropped: Arc::clone(dropped),
            }
        }
    }

    impl Read for EmptyPipe {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Ok(0)
        }
    }

    impl Drop for EmptyPipe {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn an_interrupted_rustfmt_names_the_interruption() {
        assert_eq!(
            FormattingError::Interrupted.to_string(),
            "rustfmt was interrupted"
        );
    }

    #[test]
    fn join_waits_until_the_reader_drops_its_pipe() {
        let dropped = Arc::new(AtomicBool::new(false));
        PipeReader::start(EmptyPipe::new(&dropped)).join();
        assert!(dropped.load(Ordering::SeqCst));
    }

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
