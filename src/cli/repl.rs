use std::path::PathBuf;

use super::definition_buffer::{BufferedInput, DefinitionBuffer, Replacement};
use super::eval::{EvalCommand, EvaluationSettings, FormsText, ModuleText};
use super::evaluation_output::PrintedEvaluation;
use super::exit_status::ExitStatus;
use super::prompt_lines::{EnteredText, PromptLines};
use super::terminal::{ModuleOfSources, Terminal};
use crate::build::BuildProfile;
use crate::expand::ClassifiedForm;
use crate::source::{SourceId, Span};

pub struct ReplCommand {
    root: PathBuf,
    profile: BuildProfile,
}

struct PromptSession {
    root: PathBuf,
    profile: BuildProfile,
    anchor: SourceId,
    buffer: DefinitionBuffer,
    entered: usize,
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
    pub fn new(root: PathBuf, profile: BuildProfile) -> Self {
        Self { root, profile }
    }

    pub fn run(self, terminal: &mut Terminal) -> ExitStatus {
        let mut lines = PromptLines::standard_input();
        let mut session = PromptSession::new(terminal, self.root, self.profile);
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
}

impl PromptSession {
    const MODULE: &str = "<prompt>";

    fn new(terminal: &mut Terminal, root: PathBuf, profile: BuildProfile) -> Self {
        let session = Self {
            root,
            profile,
            anchor: terminal.add_named_source(Self::MODULE, ""),
            buffer: DefinitionBuffer::default(),
            entered: 0,
        };
        session.place_prelude_sources_before_the_inputs(terminal);
        session
    }

    fn place_prelude_sources_before_the_inputs(&self, terminal: &mut Terminal) {
        terminal
            .check_sources(&self.module_of(Vec::new()), |_| false)
            .ok();
    }

    fn enter(&mut self, terminal: &mut Terminal, text: String) {
        let whole = terminal.add_named_source(&Self::input_name(self.entered + 1), &text);
        let Ok(forms) = terminal.read_source(whole) else {
            self.entered += 1;
            return;
        };
        for form in forms {
            let input = self.next_input(terminal, text[form.span().range()].to_string());
            self.enter_input(terminal, input);
        }
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

    fn enter_input(&mut self, terminal: &mut Terminal, input: EnteredInput) {
        match self.classify(terminal, &input) {
            ClassifiedInput::Expression => self.evaluate(terminal, input),
            ClassifiedInput::Definition(definition, replacement) => {
                self.define(terminal, definition, &replacement);
            }
        }
    }

    fn classify(&self, terminal: &mut Terminal, input: &EnteredInput) -> ClassifiedInput {
        let form = Span::new(input.source, 0..input.text.len());
        let mut replacement = Replacement::default();
        loop {
            let mut sources = self.buffer.sources_kept_by(&replacement);
            sources.push(input.source);
            let module = self.module_of(sources);
            let ClassifiedForm::Definition(names) = terminal.classify(&module, form) else {
                return ClassifiedInput::Expression;
            };
            match self.buffer.widened(&names, &replacement) {
                Some(wider) => replacement = wider,
                None => {
                    let source = input.source;
                    return ClassifiedInput::Definition(
                        BufferedInput { source, names },
                        replacement,
                    );
                }
            }
        }
    }

    fn define(&mut self, terminal: &mut Terminal, input: BufferedInput, replacement: &Replacement) {
        if let Some(dropped) = self.buffer.dropped_definition(&input, replacement) {
            terminal.report(&dropped.into());
            return;
        }
        let source = input.source;
        let candidate = self.buffer.replaced(replacement, input);
        let module = self.module_of(candidate.sources());
        let checked = terminal.check_sources(&module, |warning| {
            warning.primary().span().source() == source
        });
        if checked.is_ok() {
            self.buffer = candidate;
        }
    }

    fn evaluate(&self, terminal: &mut Terminal, input: EnteredInput) {
        let module = self.module_of(self.buffer.sources());
        let forms = FormsText::named(input.name, input.text, None);
        let status = EvalCommand::new(
            ModuleText::Loaded(module.path, module.sources),
            Some(self.root.clone()),
            forms,
            EvaluationSettings {
                profile: self.profile,
                output: Box::new(PrintedEvaluation),
            },
        )
        .run(terminal);
        Self::note_failed_program(terminal, status);
    }

    fn note_failed_program(terminal: &Terminal, status: ExitStatus) {
        if let ExitStatus::Program(code @ 1..) = status {
            terminal.print_note(&format!("the program exited with code {code}"));
        }
    }

    fn module_of(&self, inputs: Vec<SourceId>) -> ModuleOfSources {
        let mut sources = vec![self.anchor];
        sources.extend(inputs);
        ModuleOfSources {
            path: PathBuf::from(Self::MODULE),
            root: self.root.clone(),
            sources,
        }
    }
}
