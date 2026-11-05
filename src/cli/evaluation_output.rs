use std::hash::{BuildHasher, RandomState};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use super::diagnostic_output::JsonDiagnostics;
use super::exit_status::ExitStatus;
use super::finished_program::FinishedProgram;
use super::json_replies::{JsonObject, JsonReplies};
use super::program_stream::ProgramStream;
use super::streamed_program::StreamedProgram;
use super::terminal::Terminal;
use crate::diagnostic::Diagnostics;
use crate::expand::{ExpansionSession, ResultPrinter};
use crate::language::Language;

pub trait EvaluationOutput {
    fn attach(&self, terminal: &mut Terminal);
    fn printer(&self, session: &mut ExpansionSession<'_>) -> Result<ResultPrinter, Diagnostics>;
    fn run(&self, binary: &Path, printed_forms: Vec<usize>) -> FinishedProgram;
    fn finish(&self, status: ExitStatus);
}

pub struct PrintedEvaluation;

pub struct JsonEvaluation {
    replies: Arc<JsonReplies>,
    marker: String,
}

impl PrintedEvaluation {
    const PRINTER: &'static str = "prn";
}

impl EvaluationOutput for PrintedEvaluation {
    fn attach(&self, _terminal: &mut Terminal) {}

    fn printer(&self, session: &mut ExpansionSession<'_>) -> Result<ResultPrinter, Diagnostics> {
        let printer = session
            .prelude_value(Self::PRINTER)
            .expect("the prelude defines the printer");
        Ok(ResultPrinter::calling(printer))
    }

    fn run(&self, binary: &Path, _printed_forms: Vec<usize>) -> FinishedProgram {
        FinishedProgram::run(binary)
    }

    fn finish(&self, _status: ExitStatus) {}
}

impl JsonEvaluation {
    const VALUE_TEXT: &'static str = "pr-str";
    const VALUE_PRINTER: &'static str = "print-value";
    const VALUE_PRINTER_SOURCE: &'static str = "<value printer>";
    const STATUS: &'static str = "status";
    const DONE: &'static str = "done";
    const EXIT: &'static str = "exit";
    const RECORD_SEPARATOR: char = '\u{1e}';

    pub fn new(id: Option<String>) -> Self {
        Self {
            replies: Arc::new(JsonReplies::new(id)),
            marker: Self::random_marker(),
        }
    }

    fn random_marker() -> String {
        let random = || RandomState::new().hash_one(());
        format!(
            "{}{}{:016x}{:016x}",
            Self::RECORD_SEPARATOR,
            Language::NAME,
            random(),
            random()
        )
    }

    fn value_printer_text(&self) -> String {
        format!(
            "(fn (value) (println \"{}\" ({} value)))",
            self.marker,
            Self::VALUE_TEXT
        )
    }
}

impl EvaluationOutput for JsonEvaluation {
    fn attach(&self, terminal: &mut Terminal) {
        terminal.report_diagnostics_to(Rc::new(JsonDiagnostics::new(Arc::clone(&self.replies))));
    }

    fn printer(&self, session: &mut ExpansionSession<'_>) -> Result<ResultPrinter, Diagnostics> {
        let bound_holder = session
            .prelude_value(Self::VALUE_TEXT)
            .expect("the prelude defines the value's text");
        let source = session.add_source(Self::VALUE_PRINTER_SOURCE, &self.value_printer_text());
        let printer = session.prelude_function(Self::VALUE_PRINTER, source)?;
        Ok(ResultPrinter::through(printer, bound_holder))
    }

    fn run(&self, binary: &Path, printed_forms: Vec<usize>) -> FinishedProgram {
        let output = ProgramStream::new(&self.marker, printed_forms);
        StreamedProgram::new(Arc::clone(&self.replies)).run(binary, output)
    }

    fn finish(&self, status: ExitStatus) {
        if status == ExitStatus::UsageError {
            return;
        }
        let done = JsonObject::new(Self::STATUS, Self::DONE).with(Self::EXIT, status.code());
        let _ = self.replies.send(done);
    }
}

#[cfg(test)]
mod tests {
    use super::JsonEvaluation;
    use crate::cli::program_stream::{ProgramStream, Streamed};

    #[test]
    fn a_prompt_that_ends_like_the_language_name_streams_whole() {
        let marker = JsonEvaluation::random_marker();
        let mut stream = ProgramStream::new(&marker, vec![1]);
        assert_eq!(
            stream.read(b"Your answer"),
            [Streamed::Output(b"Your answer".to_vec())]
        );
    }
}
