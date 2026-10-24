use std::fmt;
use std::io::{self, Write};
use std::process::{Command, Stdio};

use super::token_alignment::TokenAlignment;
use crate::emit::EmittedCrate;

pub struct EmittedFormatting;

impl EmittedFormatting {
    pub fn apply(emitted: &EmittedCrate) -> Result<EmittedCrate, FormattingError> {
        let code = Rustfmt::new().format(&emitted.code)?;
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
    Misaligned,
}

impl fmt::Display for FormattingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(error) => write!(formatter, "cannot run rustfmt: {error}"),
            Self::Failed(message) if message.is_empty() => write!(formatter, "rustfmt failed"),
            Self::Failed(message) => write!(formatter, "rustfmt failed: {message}"),
            Self::Misaligned => {
                write!(
                    formatter,
                    "cannot align rustfmt's output with the emitted Rust"
                )
            }
        }
    }
}

struct Rustfmt;

impl Rustfmt {
    const PROGRAM: &'static str = "rustfmt";
    const ARGUMENTS: [&'static str; 2] = ["--edition", "2024"];

    fn new() -> Self {
        Self
    }

    fn format(&self, source: &str) -> Result<String, FormattingError> {
        let mut child = Command::new(Self::PROGRAM)
            .args(Self::ARGUMENTS)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(FormattingError::Unavailable)?;
        let mut input = child.stdin.take().expect("standard input is piped");
        input
            .write_all(source.as_bytes())
            .map_err(|error| FormattingError::Failed(error.to_string()))?;
        drop(input);
        let output = child
            .wait_with_output()
            .map_err(FormattingError::Unavailable)?;
        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(FormattingError::Failed(message));
        }
        String::from_utf8(output.stdout)
            .map_err(|_| FormattingError::Failed("rustfmt wrote non-UTF-8 output".to_string()))
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
}
