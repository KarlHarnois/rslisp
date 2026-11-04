use super::definition_buffer::{BufferedInput, DefinitionBuffer, DroppedNames, Replacement};
use super::error_location::{InputLocator, Location, ModuleLoader};
use super::name_records::NameRecords;
use super::prompt_module::PromptModule;
use super::terminal::{SourcesSurvey, Terminal};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::expand::{ExpandProblems, SetAsideInput};
use crate::source::SourceId;

pub struct DefinitionRounds<'a> {
    module: &'a PromptModule,
    before: &'a DefinitionBuffer,
    entered: BufferedInput,
    candidate: DefinitionBuffer,
    warnings: Diagnostics,
    pending: NameRecords,
}

pub enum DefinitionOutcome {
    Accepted(AcceptedDefinition),
    Rejected {
        errors: Diagnostics,
        pending: NameRecords,
    },
}

pub struct AcceptedDefinition {
    pub buffer: DefinitionBuffer,
    pub report: Diagnostics,
    pub records: NameRecords,
    pub loaders: Vec<ModuleLoader>,
}

#[derive(Default)]
struct LocatedDiagnostics {
    entered_errors: Diagnostics,
    entered_warnings: Diagnostics,
    buffered_errors: Vec<(SourceId, Diagnostic)>,
}

impl<'a> DefinitionRounds<'a> {
    pub fn new(
        module: &'a PromptModule,
        before: &'a DefinitionBuffer,
        entered: BufferedInput,
        replacement: &Replacement,
    ) -> Self {
        let dropped = before.dropped_by(replacement, &entered);
        let mut rounds = Self {
            module,
            before,
            candidate: before.replaced(replacement, entered.clone()),
            entered,
            warnings: Diagnostics::default(),
            pending: NameRecords::default(),
        };
        for names in dropped {
            rounds.set_aside_dropped(names);
        }
        rounds
    }

    pub fn run(mut self, terminal: &mut Terminal) -> DefinitionOutcome {
        loop {
            let module = self.module.of(self.candidate.sources());
            let survey = terminal.survey_sources(&module, &self.candidate.spans());
            let located = self.locate(&survey, terminal);
            if !located.entered_errors.is_empty() {
                return self.rejected(located.entered_errors);
            }
            if located.buffered_errors.is_empty() {
                return DefinitionOutcome::Accepted(
                    self.accepted(survey, located.entered_warnings),
                );
            }
            self.set_aside_round(survey, &located.buffered_errors);
        }
    }

    fn locate(&self, survey: &SourcesSurvey, terminal: &Terminal) -> LocatedDiagnostics {
        let locator = self.locator(survey);
        let mut located = LocatedDiagnostics::default();
        for diagnostic in survey.diagnostics.in_source_order() {
            let location = locator.locate(diagnostic, |source| {
                terminal.source_name(source).to_string()
            });
            located.add(location, diagnostic);
        }
        located
    }

    fn locator(&self, survey: &SourcesSurvey) -> InputLocator {
        let entered = self.entered.source();
        let buffered = self
            .candidate
            .sources()
            .into_iter()
            .filter(|source| *source != entered)
            .collect();
        let name = self.entered.name().to_string();
        InputLocator::new(buffered, name, survey.loaders.clone())
    }

    fn rejected(self, errors: Diagnostics) -> DefinitionOutcome {
        DefinitionOutcome::Rejected {
            errors,
            pending: self.pending,
        }
    }

    fn set_aside_round(&mut self, survey: SourcesSurvey, errors: &[(SourceId, Diagnostic)]) {
        for (source, error) in errors {
            let input = self
                .candidate
                .input(*source)
                .expect("an error is located at an input of the buffer")
                .clone();
            self.set_aside(&input, error);
        }
        let set_aside: Vec<SourceId> = errors.iter().map(|(source, _)| *source).collect();
        self.candidate = self.candidate.renamed(survey.names).without(&set_aside);
    }

    fn set_aside(&mut self, input: &BufferedInput, error: &Diagnostic) {
        let names: Vec<&str> = input.names().iter().map(|name| &*name.name).collect();
        let warning = ExpandProblems::set_aside(&self.set_aside_input(&names, input), error);
        self.warnings.push(warning);
        for name in input.names() {
            self.pending
                .set_aside(name, self.entered.name(), error.title());
        }
    }

    fn set_aside_dropped(&mut self, drop: DroppedNames) {
        let names: Vec<&str> = drop.dropped.iter().map(|name| &*name.name).collect();
        let first = drop.dropped[0].span;
        let input = self.set_aside_input(&names, &drop.input);
        let warning = ExpandProblems::dropped(&input, &drop.replacing, first);
        self.warnings.push(warning);
        for name in &drop.dropped {
            let reason = ExpandProblems::set_aside_reason(&drop.replacing, &name.name);
            self.pending.set_aside(name, self.entered.name(), &reason);
        }
    }

    fn set_aside_input<'n>(
        &'n self,
        names: &'n [&'n str],
        input: &'n BufferedInput,
    ) -> SetAsideInput<'n> {
        SetAsideInput::new(names, input.name(), self.entered.name())
    }

    fn accepted(self, survey: SourcesSurvey, entered_warnings: Diagnostics) -> AcceptedDefinition {
        let loaders = survey.loaders;
        let buffer = self.candidate.renamed(survey.names);
        let mut records = self.pending;
        for input in buffer.inputs() {
            Self::record_names_no_longer_bound(self.before, input, &self.entered, &mut records);
        }
        let mut report = self.warnings;
        report.extend(entered_warnings);
        AcceptedDefinition {
            buffer,
            report,
            records,
            loaders,
        }
    }

    fn record_names_no_longer_bound(
        before: &DefinitionBuffer,
        input: &BufferedInput,
        entered: &BufferedInput,
        records: &mut NameRecords,
    ) {
        let Some(earlier) = before.input(input.source()) else {
            return;
        };
        for name in earlier.names().iter().filter(|name| !input.binds(name)) {
            records.no_longer_defined(name, input.name(), entered.name());
        }
    }
}

impl LocatedDiagnostics {
    fn add(&mut self, location: Location, diagnostic: &Diagnostic) {
        match (location, diagnostic.is_error()) {
            (Location::Entered, true) => self.entered_errors.push(diagnostic.clone()),
            (Location::Entered, false) => self.entered_warnings.push(diagnostic.clone()),
            (Location::Buffered(source), true) => self.add_buffered_error(source, diagnostic),
            (Location::Buffered(_), false) => {}
        }
    }

    fn add_buffered_error(&mut self, source: SourceId, error: &Diagnostic) {
        if !self.buffered_errors.iter().any(|(held, _)| *held == source) {
            self.buffered_errors.push((source, error.clone()));
        }
    }
}
