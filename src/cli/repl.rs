use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use super::definition_buffer::{BufferedInput, DefinitionBuffer, Replacement};
use super::definition_rounds::{AcceptedDefinition, DefinitionOutcome, DefinitionRounds};
use super::error_location::{InputLocator, ModuleLoader};
use super::eval::{EvalCommand, EvaluationSettings, FormsText, ModuleText};
use super::evaluation_output::PrintedEvaluation;
use super::exit_status::ExitStatus;
use super::interruption::Interruption;
use super::name_records::NameRecords;
use super::prompt_diagnostics::{PromptAnnotations, PromptDiagnostics};
use super::prompt_lines::{EnteredText, PromptLines};
use super::prompt_module::PromptModule;
use super::terminal::Terminal;
use crate::build::BuildProfile;
use crate::diagnostic::Diagnostics;
use crate::expand::ClassifiedForm;
use crate::source::{SourceId, Span};

pub struct ReplCommand {
    root: PathBuf,
    profile: BuildProfile,
}

struct PromptSession {
    module: PromptModule,
    profile: BuildProfile,
    buffer: DefinitionBuffer,
    annotations: Rc<RefCell<PromptAnnotations>>,
    loaders: Vec<ModuleLoader>,
    entered: usize,
    interruption: Interruption,
}

struct EnteredInput {
    name: String,
    text: String,
    source: SourceId,
}

enum ClassifiedInput {
    Expression,
    Definition(BufferedInput, Replacement),
}

impl ReplCommand {
    pub const DEFAULT_ROOT: &str = ".";

    pub fn new(root: PathBuf, profile: BuildProfile) -> Self {
        Self { root, profile }
    }

    pub fn run(self, terminal: &mut Terminal) -> ExitStatus {
        let mut lines = match PromptLines::standard_input(terminal.diagnostic_output()) {
            Ok(lines) => lines,
            Err(error) => return ExitStatus::report_unreadable_input(&error),
        };
        let interruption = lines
            .catch_interrupts()
            .unwrap_or_else(|error| Self::note_uncaught_interrupts(terminal, &error));
        terminal.stop_before_rustc_on(interruption.clone());
        let session = PromptSession::new(terminal, self.root, self.profile, interruption);
        Self::answer(&mut lines, session, terminal)
    }

    fn answer(
        lines: &mut PromptLines,
        mut session: PromptSession,
        terminal: &mut Terminal,
    ) -> ExitStatus {
        loop {
            match lines.next_text() {
                Ok(Some(EnteredText::Complete(text))) => session.enter(terminal, text),
                Ok(Some(EnteredText::Unfinished(text))) => {
                    session.report_unfinished(terminal, text);
                    return ExitStatus::Success;
                }
                Ok(None) => return ExitStatus::Success,
                Err(error) => return ExitStatus::report_unreadable_input(&error),
            }
        }
    }

    fn note_uncaught_interrupts(terminal: &Terminal, error: &ctrlc::Error) -> Interruption {
        terminal.print_note(&format!("Ctrl-C will end the session: {error}"));
        Interruption::default()
    }
}

impl PromptSession {
    const INTERRUPTED: &str = "interrupted";

    fn new(
        terminal: &mut Terminal,
        root: PathBuf,
        profile: BuildProfile,
        interruption: Interruption,
    ) -> Self {
        let anchor = terminal.add_named_source(PromptModule::PATH, "");
        let session = Self {
            module: PromptModule::new(anchor, root),
            profile,
            buffer: DefinitionBuffer::default(),
            annotations: Rc::default(),
            loaders: Vec::new(),
            entered: 0,
            interruption,
        };
        session.place_prelude_sources_before_the_inputs(terminal);
        session.annotate_diagnostics(terminal);
        session
    }

    fn place_prelude_sources_before_the_inputs(&self, terminal: &mut Terminal) {
        terminal
            .check_sources(&self.module.of(Vec::new()), |_| false)
            .ok();
    }

    fn annotate_diagnostics(&self, terminal: &mut Terminal) {
        let annotated =
            PromptDiagnostics::new(terminal.diagnostic_output(), Rc::clone(&self.annotations));
        terminal.report_diagnostics_to(Rc::new(annotated));
    }

    fn enter(&mut self, terminal: &mut Terminal, text: String) {
        let whole = terminal.add_named_source(&Self::input_name(self.entered + 1), &text);
        let Ok(forms) = terminal.read_source(whole) else {
            self.entered += 1;
            return;
        };
        for form in forms {
            let input = self.next_input(terminal, text[form.span().range()].to_string());
            self.interruption.clear();
            if self.enter_input(terminal, input) == ExitStatus::Interrupted {
                return self.acknowledge_interruption();
            }
        }
    }

    fn acknowledge_interruption(&self) {
        self.interruption.acknowledge();
        eprintln!("{}", Self::INTERRUPTED);
    }

    fn report_unfinished(&mut self, terminal: &mut Terminal, text: String) {
        let input = self.next_input(terminal, text);
        terminal.read_source(input.source).ok();
    }

    fn next_input(&mut self, terminal: &mut Terminal, text: String) -> EnteredInput {
        self.entered += 1;
        let name = Self::input_name(self.entered);
        let source = terminal.add_named_source(&name, &text);
        EnteredInput { name, text, source }
    }

    fn input_name(number: usize) -> String {
        format!("<input {number}>")
    }

    fn enter_input(&mut self, terminal: &mut Terminal, input: EnteredInput) -> ExitStatus {
        match self.classify(terminal, &input) {
            ClassifiedInput::Expression => self.evaluate(terminal, input),
            ClassifiedInput::Definition(definition, replacement) => {
                self.define(terminal, definition, &replacement);
                ExitStatus::Success
            }
        }
    }

    fn classify(&self, terminal: &mut Terminal, input: &EnteredInput) -> ClassifiedInput {
        let form = Span::new(input.source, 0..input.text.len());
        let mut replacement = Replacement::default();
        loop {
            let mut sources = self.buffer.sources_kept_by(&replacement);
            sources.push(input.source);
            let module = self.module.of(sources);
            let ClassifiedForm::Definition(names) = terminal.classify(&module, form) else {
                return ClassifiedInput::Expression;
            };
            match self.buffer.widened(&names, &replacement) {
                Some(wider) => replacement = wider,
                None => {
                    let buffered = BufferedInput::new(input.name.clone(), form, names);
                    return ClassifiedInput::Definition(buffered, replacement);
                }
            }
        }
    }

    fn define(&mut self, terminal: &mut Terminal, input: BufferedInput, replacement: &Replacement) {
        let rounds = DefinitionRounds::new(&self.module, &self.buffer, input, replacement);
        match rounds.run(terminal) {
            DefinitionOutcome::Accepted(accepted) => self.accept(terminal, accepted),
            DefinitionOutcome::Rejected { errors, pending } => {
                self.reject(terminal, &errors, pending)
            }
        }
    }

    fn accept(&mut self, terminal: &Terminal, accepted: AcceptedDefinition) {
        self.annotations
            .borrow_mut()
            .commit(accepted.records, accepted.buffer.bound_names());
        self.buffer = accepted.buffer;
        self.loaders = accepted.loaders;
        terminal.print_diagnostics(&accepted.report);
    }

    fn reject(&self, terminal: &Terminal, errors: &Diagnostics, pending: NameRecords) {
        self.annotations.borrow_mut().hold_pending(pending);
        terminal.report(errors);
        self.annotations.borrow_mut().drop_pending();
    }

    fn evaluate(&self, terminal: &mut Terminal, input: EnteredInput) -> ExitStatus {
        let module = self.module.of(self.buffer.sources());
        self.locate_expression_errors(&input.name);
        let forms = FormsText::named(input.name, input.text, None);
        let status = EvalCommand::new(
            ModuleText::Loaded(module.path, module.sources),
            Some(self.module.root().to_path_buf()),
            forms,
            EvaluationSettings {
                profile: self.profile,
                output: Box::new(PrintedEvaluation::for_standard_output()),
            },
        )
        .run(terminal);
        self.annotations.borrow_mut().forget_expression();
        Self::note_failed_program(terminal, status);
        status
    }

    fn locate_expression_errors(&self, expression: &str) {
        let locator = InputLocator::new(
            self.buffer.sources(),
            expression.to_string(),
            self.loaders.clone(),
        );
        self.annotations.borrow_mut().locate_expression(locator);
    }

    fn note_failed_program(terminal: &Terminal, status: ExitStatus) {
        if let ExitStatus::Program(code @ 1..) = status {
            terminal.print_note(&format!("the program exited with code {code}"));
        }
    }
}
