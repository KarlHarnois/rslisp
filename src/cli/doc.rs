use std::path::PathBuf;

use super::exit_status::ExitStatus;
use super::terminal::Terminal;
use crate::core::{Expr, ExprKind, Leaf, Literal};
use crate::language::Language;
use crate::reader::{Form, FormKind, Symbol};
use crate::source::SourceId;

pub struct DocCommand {
    environment: DocEnvironment,
    term: String,
}

impl DocCommand {
    pub const NAME: &'static str = "doc";

    pub fn new(environment: DocEnvironment, term: String) -> Self {
        Self { environment, term }
    }

    pub fn run(self) -> ExitStatus {
        let mut terminal = Terminal::new();
        match DocLookup::new(&mut terminal).describe(&self.environment, &self.term) {
            Ok(page) => {
                println!("{page}");
                ExitStatus::Success
            }
            Err(status) => status,
        }
    }
}

pub enum DocEnvironment {
    Prelude,
    Program(PathBuf),
}

impl DocEnvironment {
    const PRELUDE_ENTRY: &'static str = "prelude-only";

    pub fn new(file: Option<PathBuf>) -> Self {
        file.map_or(Self::Prelude, Self::Program)
    }

    fn entry(&self) -> PathBuf {
        match self {
            Self::Prelude => PathBuf::from(Self::PRELUDE_ENTRY).with_extension(Language::EXTENSION),
            Self::Program(path) => path.clone(),
        }
    }

    fn load(&self, terminal: &mut Terminal) -> Result<SourceId, ExitStatus> {
        match self {
            Self::Prelude => Ok(terminal.add_empty_source(&self.entry())),
            Self::Program(path) => terminal.load(path),
        }
    }
}

pub struct DocLookup<'t> {
    terminal: &'t mut Terminal,
}

impl<'t> DocLookup<'t> {
    const TERM_SOURCE: &'static str = "<term>";

    pub fn new(terminal: &'t mut Terminal) -> Self {
        Self { terminal }
    }

    pub fn describe(
        &mut self,
        environment: &DocEnvironment,
        term: &str,
    ) -> Result<String, ExitStatus> {
        let lookup = Self::lookup_call(self.read_term(term)?);
        let entry = environment.load(self.terminal)?;
        let expanded = self
            .terminal
            .expand_expression(&environment.entry(), entry, lookup)?;
        Self::page(&expanded)
    }

    fn read_term(&mut self, term: &str) -> Result<Form, ExitStatus> {
        let forms = self.terminal.read_text(Self::TERM_SOURCE, term)?;
        match forms.as_slice() {
            [form] if form.as_symbol().is_some() => Ok(form.clone()),
            _ => {
                eprintln!("error: `{term}` is not one name");
                Err(ExitStatus::UsageError)
            }
        }
    }

    fn lookup_call(term: Form) -> Form {
        let span = term.span();
        let head = Form::new(FormKind::Symbol(Symbol::new(DocCommand::NAME)), span);
        Form::new(FormKind::List(vec![head, term]), span)
    }

    fn page(expanded: &Expr) -> Result<String, ExitStatus> {
        match expanded.kind() {
            ExprKind::Leaf(Leaf::Literal(Literal::String(text))) => Ok(text.to_string()),
            _ => {
                eprintln!("error: `{}` did not expand to a string", DocCommand::NAME);
                Err(ExitStatus::ProgramErrors)
            }
        }
    }
}
