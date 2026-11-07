use std::ops::Range;
use std::path::{Path, PathBuf};

use super::evaluation_output::EvaluationOutput;
use super::exit_status::ExitStatus;
use super::standard_error::StandardError;
use super::temporary_directory::TemporaryDirectory;
use super::terminal::{BuildTarget, EvaluationRequest, ModuleOfSources, Terminal};
use crate::build::BuildProfile;
use crate::check::ProgramChecker;
use crate::diagnostic::{Diagnostic, DiagnosticCode, Diagnostics};
use crate::emit::EmittedCrate;
use crate::expand::{ExpandProblems, ExpansionSession, GeneratedEntry, Program, ResultPrinter};
use crate::modules::ModuleSources;
use crate::reader::Form;
use crate::source::{SourceId, Span, TextPosition};
use crate::types::{FormResult, TypedProgram};

pub struct EvalCommand {
    module: ModuleText,
    root: Option<PathBuf>,
    forms: FormsText,
    profile: BuildProfile,
    output: Box<dyn EvaluationOutput>,
}

pub struct EvaluationSettings {
    pub profile: BuildProfile,
    pub output: Box<dyn EvaluationOutput>,
}

pub enum ModuleText {
    File(PathBuf),
    StandardInput(PathBuf),
    Loaded(PathBuf, Vec<SourceId>),
}

pub struct FormsText {
    name: String,
    text: String,
    position: Option<TextPosition>,
}

impl EvalCommand {
    pub fn new(
        module: ModuleText,
        root: Option<PathBuf>,
        forms: FormsText,
        settings: EvaluationSettings,
    ) -> Self {
        Self {
            module,
            root,
            forms,
            profile: settings.profile,
            output: settings.output,
        }
    }

    pub fn run(self, terminal: &mut Terminal) -> ExitStatus {
        self.output.attach(terminal);
        let status = match self.evaluate(terminal) {
            Ok(status) | Err(status) => status,
        };
        self.output.finish(status);
        status
    }

    fn evaluate(&self, terminal: &mut Terminal) -> Result<ExitStatus, ExitStatus> {
        let module = self.load_module(terminal)?;
        let anchor = module.anchor();
        let ranges = self.forms.ranges(terminal, self.module.path(), anchor)?;
        let expanded = self.expand(terminal, module, &ranges)?;
        let module_warnings = self.module.check(terminal, &expanded.program)?.warnings;
        terminal.print_diagnostics(&expanded.definition_notes());
        let Some(entry) = expanded.entry(terminal)? else {
            return Ok(ExitStatus::Success);
        };
        let enclosing = self.forms.enclosing(anchor);
        let shown = ShownWarnings::new(expanded.forms_source, enclosing, module_warnings);
        let checked = expanded.checked_with_printing(terminal, &entry, &shown)?;
        let emitted = terminal.emit_checked(&checked.program)?;
        self.build_and_run(terminal, &emitted, checked.printed_forms)
    }

    fn expand(
        &self,
        terminal: &mut Terminal,
        module: ModuleOfSources,
        ranges: &[Range<usize>],
    ) -> Result<ExpandedEvaluation, ExitStatus> {
        let request = EvaluationRequest {
            evaluated: self.forms.module_spans(module.anchor(), ranges),
            module,
        };
        terminal.within_evaluation(&request, |session| {
            ExpandedEvaluation::new(session, &request, &self.forms, self.output.as_ref())
        })
    }

    fn load_module(&self, terminal: &mut Terminal) -> Result<ModuleOfSources, ExitStatus> {
        Ok(ModuleOfSources {
            path: self.module.path().to_path_buf(),
            root: self.root(),
            sources: self.module.load(terminal)?,
        })
    }

    fn build_and_run(
        &self,
        terminal: &mut Terminal,
        emitted: &EmittedCrate,
        printed_forms: Vec<usize>,
    ) -> Result<ExitStatus, ExitStatus> {
        let directory = TemporaryDirectory::create().map_err(Self::report_no_build_directory)?;
        let crate_name = Terminal::crate_name(self.module.path());
        let binary = directory.path().join(&crate_name);
        let target = BuildTarget {
            directory: directory.path().to_path_buf(),
            crate_name: &crate_name,
            output: &binary,
        };
        terminal.compile(emitted, &target, self.profile)?;
        Ok(self.output.run(&binary, printed_forms).shell_status())
    }

    fn root(&self) -> PathBuf {
        self.root
            .clone()
            .unwrap_or_else(|| Terminal::directory_of(self.module.path()))
    }

    fn report_no_build_directory(error: std::io::Error) -> ExitStatus {
        StandardError::write_line(format_args!(
            "error: cannot create a build directory: {error}"
        ));
        ExitStatus::Environment
    }
}

impl ModuleText {
    fn path(&self) -> &Path {
        match self {
            Self::File(path) | Self::StandardInput(path) | Self::Loaded(path, _) => path,
        }
    }

    fn load(&self, terminal: &mut Terminal) -> Result<Vec<SourceId>, ExitStatus> {
        match self {
            Self::File(path) => Ok(vec![terminal.load(path)?]),
            Self::StandardInput(path) => match ModuleSources::read_standard_input() {
                Ok(text) => Ok(vec![terminal.add_source(path, text)]),
                Err(error) => Err(ExitStatus::report_unreadable_input(&error)),
            },
            Self::Loaded(_, sources) => Ok(sources.clone()),
        }
    }

    fn check(
        &self,
        terminal: &mut Terminal,
        program: &Program,
    ) -> Result<TypedProgram, ExitStatus> {
        match self {
            Self::Loaded(..) => terminal.check_showing(program, |_| false),
            Self::File(_) | Self::StandardInput(_) => terminal.check_expanded(program),
        }
    }
}

impl FormsText {
    const SOURCE_NAME: &'static str = "<eval>";

    pub fn new(text: String, position: Option<TextPosition>) -> Self {
        Self::named(Self::SOURCE_NAME.to_string(), text, position)
    }

    pub fn named(name: String, text: String, position: Option<TextPosition>) -> Self {
        Self {
            name,
            text,
            position,
        }
    }

    fn ranges(
        &self,
        terminal: &Terminal,
        module: &Path,
        module_source: SourceId,
    ) -> Result<Vec<Range<usize>>, ExitStatus> {
        let mut scratch = Terminal::reporting_to(terminal.diagnostic_output());
        let enclosing = scratch.add_source(module, terminal.text_of(module_source));
        let source = self.add_to(&mut scratch, enclosing, module)?;
        let forms = scratch.read_source(source)?;
        Ok(forms.iter().map(|form| form.span().range()).collect())
    }

    fn add_to(
        &self,
        scratch: &mut Terminal,
        enclosing: SourceId,
        module: &Path,
    ) -> Result<SourceId, ExitStatus> {
        let Some(position) = self.position else {
            return Ok(scratch.add_named_source(&self.name, &self.text));
        };
        scratch
            .add_source_inside(enclosing, position, &self.text)
            .ok_or_else(|| Self::report_misplaced(module, position))
    }

    fn add_to_session(&self, session: &mut ExpansionSession<'_>, module: SourceId) -> SourceId {
        match self.position {
            Some(position) => session
                .add_source_inside(module, position, &self.text)
                .expect("the forms were found at their position before expansion"),
            None => session.add_source(&self.name, &self.text),
        }
    }

    fn enclosing(&self, module: SourceId) -> Option<SourceId> {
        self.position.map(|_| module)
    }

    fn module_spans(&self, module: SourceId, ranges: &[Range<usize>]) -> Vec<Span> {
        if self.position.is_none() {
            return Vec::new();
        }
        ranges
            .iter()
            .map(|range| Span::new(module, range.clone()))
            .collect()
    }

    fn report_misplaced(module: &Path, position: TextPosition) -> ExitStatus {
        StandardError::write_line(format_args!(
            "error: the forms are not the text of `{}` at {position}",
            module.display()
        ));
        ExitStatus::UsageError
    }
}

struct ExpandedEvaluation {
    program: Program,
    forms_source: SourceId,
    entry: Option<Result<GeneratedEntry, Diagnostics>>,
    definitions: Vec<Span>,
    printer: ResultPrinter,
    form_numbers: Vec<usize>,
}

struct CheckedEvaluation {
    program: TypedProgram,
    printed_forms: Vec<usize>,
}

impl ExpandedEvaluation {
    fn new(
        mut session: ExpansionSession<'_>,
        request: &EvaluationRequest,
        text: &FormsText,
        output: &dyn EvaluationOutput,
    ) -> Result<Self, Diagnostics> {
        let forms_source = text.add_to_session(&mut session, request.module.anchor());
        let forms = session.read(forms_source)?;
        let definitions = Self::definitions(&session, request);
        let (form_numbers, running): (Vec<usize>, Vec<Form>) = (1..)
            .zip(forms)
            .filter(|(_, form)| !Self::is_definition(&definitions, form))
            .unzip();
        let entry = Self::generated_entry(&mut session, &running);
        let printer = output.printer(&mut session)?;
        Ok(Self {
            program: session.finish()?,
            forms_source,
            entry,
            definitions,
            printer,
            form_numbers,
        })
    }

    fn definitions(session: &ExpansionSession<'_>, request: &EvaluationRequest) -> Vec<Span> {
        request
            .evaluated
            .iter()
            .copied()
            .filter(|span| session.defines(*span))
            .collect()
    }

    fn is_definition(definitions: &[Span], form: &Form) -> bool {
        definitions
            .iter()
            .any(|definition| definition.range() == form.span().range())
    }

    fn generated_entry(
        session: &mut ExpansionSession<'_>,
        running: &[Form],
    ) -> Option<Result<GeneratedEntry, Diagnostics>> {
        let span = Self::span_of(running)?;
        Some(session.generated_entry(running, span))
    }

    fn span_of(forms: &[Form]) -> Option<Span> {
        let first = forms.first()?.span();
        let last = forms.last()?.span();
        Some(Span::new(
            first.source(),
            first.range().start..last.range().end,
        ))
    }

    fn definition_notes(&self) -> Diagnostics {
        let mut notes = Diagnostics::default();
        for span in &self.definitions {
            notes.push(ExpandProblems::evaluated_definition(*span));
        }
        notes
    }

    fn entry(&self, terminal: &Terminal) -> Result<Option<GeneratedEntry>, ExitStatus> {
        self.entry
            .clone()
            .transpose()
            .map_err(|diagnostics| terminal.report(&diagnostics))
    }

    fn checked_with_printing(
        self,
        terminal: &mut Terminal,
        entry: &GeneratedEntry,
        shown: &ShownWarnings,
    ) -> Result<CheckedEvaluation, ExitStatus> {
        let mut program = self.program;
        program.enter(entry);
        program.check_also(self.printer.bound_holder());
        let results = ProgramChecker::form_results(&program, self.printer.bound_holder())
            .map_err(|diagnostics| terminal.report(&diagnostics))?;
        terminal.print_diagnostics(&Self::result_notes(&results));
        let printed = Self::printed(&results);
        program.enter(&entry.printing(&self.printer, &printed));
        let printed_forms = printed
            .iter()
            .map(|position| self.form_numbers[*position])
            .collect();
        Ok(CheckedEvaluation {
            program: terminal.check_showing(&program, |warning| shown.shows(warning))?,
            printed_forms,
        })
    }

    fn result_notes(results: &[FormResult]) -> Diagnostics {
        let mut notes = Diagnostics::default();
        for result in results {
            if let FormResult::Unprintable(note) = result {
                notes.push(note.clone());
            }
        }
        notes
    }

    fn printed(results: &[FormResult]) -> Vec<usize> {
        results
            .iter()
            .enumerate()
            .filter(|(_, result)| matches!(result, FormResult::Printable))
            .map(|(position, _)| position)
            .collect()
    }
}

struct ShownWarnings {
    forms_source: SourceId,
    enclosing: Option<SourceId>,
    module_warnings: Vec<(Option<DiagnosticCode>, Span)>,
}

impl ShownWarnings {
    fn new(
        forms_source: SourceId,
        enclosing: Option<SourceId>,
        module_warnings: Diagnostics,
    ) -> Self {
        Self {
            forms_source,
            enclosing,
            module_warnings: module_warnings
                .in_source_order()
                .into_iter()
                .map(|warning| (warning.code().cloned(), warning.primary().span()))
                .collect(),
        }
    }

    fn shows(&self, warning: &Diagnostic) -> bool {
        warning.primary().span().source() == self.forms_source && !self.module_reported(warning)
    }

    fn module_reported(&self, warning: &Diagnostic) -> bool {
        let Some(enclosing) = self.enclosing else {
            return false;
        };
        let place = Span::new(enclosing, warning.primary().span().range());
        self.module_warnings
            .contains(&(warning.code().cloned(), place))
    }
}
