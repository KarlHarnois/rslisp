use std::path::PathBuf;

use super::exit_status::ExitStatus;
use super::terminal::Terminal;
use crate::core::{BindingId, Expr, ExprKind, Leaf, Literal};
use crate::diagnostic::Diagnostics;
use crate::expand::{ExpansionSession, Program};
use crate::language::Language;
use crate::reader::{Form, FormKind, Symbol};
use crate::source::{SourceId, Span};
use crate::types::TypedProgram;

pub struct DocCommand {
    environment: DocEnvironment,
    term: String,
    page: DocPage,
}

pub enum DocPage {
    Written,
    Typed,
}

impl DocCommand {
    pub const NAME: &'static str = "doc";

    pub fn new(environment: DocEnvironment, term: String, page: DocPage) -> Self {
        Self {
            environment,
            term,
            page,
        }
    }

    pub fn run(self, terminal: &mut Terminal) -> ExitStatus {
        let mut lookup = DocLookup::new(terminal);
        let output = match self.page {
            DocPage::Written => lookup
                .describe(&self.environment, &self.term)
                .map(DocOutput::page),
            DocPage::Typed => lookup.describe_with_type(&self.environment, &self.term),
        };
        match output {
            Ok(output) => output.print(terminal),
            Err(status) => status,
        }
    }
}

struct DocOutput {
    type_line: Option<String>,
    page: String,
    status: ExitStatus,
}

impl DocOutput {
    fn page(page: String) -> Self {
        Self {
            type_line: None,
            page,
            status: ExitStatus::Success,
        }
    }

    fn typed(page: String, type_line: Result<Option<String>, ExitStatus>) -> Self {
        match type_line {
            Ok(type_line) => Self {
                type_line,
                ..Self::page(page)
            },
            Err(status) => Self {
                status,
                ..Self::page(page)
            },
        }
    }

    fn print(&self, terminal: &Terminal) -> ExitStatus {
        match terminal.write_output(self.text()) {
            ExitStatus::Success => self.status,
            failure => failure,
        }
    }

    fn text(&self) -> String {
        self.type_line
            .iter()
            .chain([&self.page])
            .map(|line| format!("{line}\n"))
            .collect()
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
        let read = Self::read_term(term)?;
        let entry = environment.load(self.terminal)?;
        let expanded = self.terminal.expand_expression(
            &environment.entry(),
            entry,
            (Self::TERM_SOURCE, term),
            |source| Self::lookup_call(&read, source),
        )?;
        Self::page(&expanded)
    }

    fn describe_with_type(
        &mut self,
        environment: &DocEnvironment,
        term: &str,
    ) -> Result<DocOutput, ExitStatus> {
        let read = Self::read_term(term)?;
        let entry = environment.load(self.terminal)?;
        let lookup = self
            .terminal
            .within_session(&environment.entry(), entry, |session| {
                TypedLookup::new(session, (Self::TERM_SOURCE, term)).look_up(&read)
            })?;
        let page = Self::page(&lookup.page)?;
        let type_line = self
            .terminal
            .check_expanded(&lookup.program)
            .map(|typed| lookup.type_line(&typed));
        Ok(DocOutput::typed(page, type_line))
    }

    fn read_term(term: &str) -> Result<Form, ExitStatus> {
        let forms = Terminal::new()
            .read_text(Self::TERM_SOURCE, term)
            .map_err(|_| ExitStatus::UsageError)?;
        match forms.as_slice() {
            [form] if form.as_symbol().is_some() => Ok(form.clone()),
            _ => {
                eprintln!("error: `{term}` is not one name");
                Err(ExitStatus::UsageError)
            }
        }
    }

    fn lookup_call(term: &Form, source: SourceId) -> Form {
        let span = Span::new(source, term.span().range());
        let head = Form::new(FormKind::Symbol(Symbol::new(DocCommand::NAME)), span);
        let name = Form::new(term.kind().clone(), span);
        Form::new(FormKind::List(vec![head, name]), span)
    }

    fn page(expanded: &Expr) -> Result<String, ExitStatus> {
        match Self::page_text(expanded) {
            Some(text) => Ok(text.to_string()),
            None => {
                eprintln!("error: `{}` did not expand to a string", DocCommand::NAME);
                Err(ExitStatus::ProgramErrors)
            }
        }
    }

    fn page_text(expanded: &Expr) -> Option<&str> {
        match expanded.kind() {
            ExprKind::Leaf(Leaf::Literal(Literal::String(text))) => Some(text),
            _ => None,
        }
    }
}

struct TypedLookup<'s> {
    session: ExpansionSession<'s>,
    source: SourceId,
}

impl<'s> TypedLookup<'s> {
    fn new(mut session: ExpansionSession<'s>, (name, text): (&str, &str)) -> Self {
        let source = session.add_source(name, text);
        Self { session, source }
    }

    fn look_up(mut self, term: &Form) -> Result<TypedPage, Diagnostics> {
        let page = self.expand_page(term)?;
        let signed = self.signed_binding(term, &page)?;
        Ok(TypedPage {
            page,
            signed,
            program: self.session.finish()?,
        })
    }

    fn signed_binding(
        &mut self,
        term: &Form,
        page: &Expr,
    ) -> Result<Option<BindingId>, Diagnostics> {
        let binding = self.session.binding(term);
        let Some(value_term) = self.session.hide_type(term) else {
            return Ok(binding);
        };
        let value_page = self.expand_page(&value_term)?;
        let describes_the_value = DocLookup::page_text(&value_page) == DocLookup::page_text(page);
        Ok(if describes_the_value { binding } else { None })
    }

    fn expand_page(&mut self, term: &Form) -> Result<Expr, Diagnostics> {
        self.session
            .expression(DocLookup::lookup_call(term, self.source))
    }
}

struct TypedPage {
    page: Expr,
    signed: Option<BindingId>,
    program: Program,
}

impl TypedPage {
    fn type_line(&self, typed: &TypedProgram) -> Option<String> {
        typed.signature(self.signed?)
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

    fn load(&self, terminal: &mut Terminal) -> Result<SourceId, ExitStatus> {
        match self {
            Self::Prelude => Ok(terminal.add_empty_source(&self.entry())),
            Self::Program(path) => terminal.load(path),
        }
    }

    fn entry(&self) -> PathBuf {
        match self {
            Self::Prelude => PathBuf::from(Self::PRELUDE_ENTRY).with_extension(Language::EXTENSION),
            Self::Program(path) => path.clone(),
        }
    }
}
