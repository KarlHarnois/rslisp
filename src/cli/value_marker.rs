use std::hash::{BuildHasher, RandomState};

use super::program_stream::ProgramStream;
use crate::diagnostic::Diagnostics;
use crate::expand::{ExpansionSession, ResultPrinter};
use crate::language::Language;

pub struct ValueMarker {
    marker: String,
}

impl ValueMarker {
    const VALUE_TEXT: &'static str = "pr-str";
    const VALUE_PRINTER: &'static str = "print-value";
    const VALUE_PRINTER_SOURCE: &'static str = "<value printer>";
    const RECORD_SEPARATOR: char = '\u{1e}';

    pub fn random() -> Self {
        let random = || RandomState::new().hash_one(());
        Self {
            marker: format!(
                "{}{}{:016x}{:016x}",
                Self::RECORD_SEPARATOR,
                Language::NAME,
                random(),
                random()
            ),
        }
    }

    pub fn printer(
        &self,
        session: &mut ExpansionSession<'_>,
    ) -> Result<ResultPrinter, Diagnostics> {
        let bound_holder = session
            .prelude_value(Self::VALUE_TEXT)
            .expect("the prelude defines the value's text");
        let source = session.add_source(Self::VALUE_PRINTER_SOURCE, &self.printer_text());
        let printer = session.prelude_function(Self::VALUE_PRINTER, source)?;
        Ok(ResultPrinter::through(printer, bound_holder))
    }

    pub fn text(&self) -> &str {
        &self.marker
    }

    pub fn records(&self) -> ProgramStream {
        ProgramStream::recording(&self.marker)
    }

    pub fn stream(&self, printed_forms: Vec<usize>) -> ProgramStream {
        ProgramStream::new(&self.marker, printed_forms)
    }

    fn printer_text(&self) -> String {
        format!(
            "(fn (value) (println \"{}\" ({} value)))",
            self.marker,
            Self::VALUE_TEXT
        )
    }
}

#[cfg(test)]
mod tests {
    use super::ValueMarker;
    use crate::cli::program_stream::Streamed;

    #[test]
    fn a_prompt_that_ends_like_the_language_name_streams_whole() {
        let mut stream = ValueMarker::random().stream(vec![1]);
        assert_eq!(
            stream.read(b"Your answer"),
            [Streamed::Output(b"Your answer".to_vec())]
        );
    }
}
