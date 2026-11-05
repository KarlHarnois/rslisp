use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use super::diagnostic_output::JsonDiagnostics;
use super::exit_status::ExitStatus;
use super::finished_program::FinishedProgram;
use super::json_replies::{JsonObject, JsonReplies};
use super::streamed_program::StreamedProgram;
use super::terminal::Terminal;
use super::value_marker::ValueMarker;
use crate::diagnostic::Diagnostics;
use crate::expand::{ExpansionSession, ResultPrinter};

pub trait EvaluationOutput {
    fn attach(&self, terminal: &mut Terminal);
    fn printer(&self, session: &mut ExpansionSession<'_>) -> Result<ResultPrinter, Diagnostics>;
    fn run(&self, binary: &Path, printed_forms: Vec<usize>) -> FinishedProgram;
    fn finish(&self, status: ExitStatus);
}

pub struct PrintedEvaluation;

pub struct JsonEvaluation {
    replies: Arc<JsonReplies>,
    marker: ValueMarker,
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
    const STATUS: &'static str = "status";
    const DONE: &'static str = "done";
    const EXIT: &'static str = "exit";

    pub fn new(id: Option<String>) -> Self {
        Self {
            replies: Arc::new(JsonReplies::new(id)),
            marker: ValueMarker::random(),
        }
    }
}

impl EvaluationOutput for JsonEvaluation {
    fn attach(&self, terminal: &mut Terminal) {
        terminal.report_diagnostics_to(Rc::new(JsonDiagnostics::new(Arc::clone(&self.replies))));
    }

    fn printer(&self, session: &mut ExpansionSession<'_>) -> Result<ResultPrinter, Diagnostics> {
        self.marker.printer(session)
    }

    fn run(&self, binary: &Path, printed_forms: Vec<usize>) -> FinishedProgram {
        let output = self.marker.stream(printed_forms);
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
