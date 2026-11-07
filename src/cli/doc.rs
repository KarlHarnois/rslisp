use std::io;
use std::path::PathBuf;

use anstream::AutoStream;

use super::exit_status::ExitStatus;
use super::standard_error::StandardError;
use super::terminal::Terminal;
use super::token_colors::TokenColors;
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
    const SEPARATOR: &'static str = "--";

    pub fn usage() -> String {
        let command = format!("{} {}", Language::NAME, Self::NAME);
        format!(
            "{command} [OPTIONS] <TERM>\n       {command} [OPTIONS] {} <TERM>",
            Self::SEPARATOR
        )
    }

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
        let colors = TokenColors::deciding(AutoStream::choice(&io::stdout()));
        match output {
            Ok(output) => output.print(terminal, colors),
            Err(status) => status,
        }
    }
}

struct PageText {
    header: String,
    body: String,
}

impl PageText {
    fn painted(&self, colors: TokenColors) -> String {
        format!("{}{}", colors.paint(&self.header), self.body)
    }
}

struct DocOutput {
    type_line: Option<String>,
    page: PageText,
    status: ExitStatus,
}

impl DocOutput {
    fn page(page: PageText) -> Self {
        Self {
            type_line: None,
            page,
            status: ExitStatus::Success,
        }
    }

    fn typed(page: PageText, type_line: Result<Option<String>, ExitStatus>) -> Self {
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

    fn print(&self, terminal: &Terminal, colors: TokenColors) -> ExitStatus {
        match terminal.write_output(self.text(colors)) {
            ExitStatus::Success => self.status,
            failure => failure,
        }
    }

    fn text(&self, colors: TokenColors) -> String {
        self.type_line
            .iter()
            .map(|type_line| colors.paint(type_line))
            .chain([self.page.painted(colors)])
            .map(|line| format!("{line}\n"))
            .collect()
    }
}

pub struct DocLookup<'t> {
    terminal: &'t mut Terminal,
}

impl<'t> DocLookup<'t> {
    const TERM_SOURCE: &'static str = "<term>";
    const HEADER_MACRO: &'static str = "header";
    const BODY_MACRO: &'static str = "page-body";

    pub fn new(terminal: &'t mut Terminal) -> Self {
        Self { terminal }
    }

    fn describe(
        &mut self,
        environment: &DocEnvironment,
        term: &str,
    ) -> Result<PageText, ExitStatus> {
        let read = self.read_term(term)?;
        let entry = environment.load(self.terminal)?;
        let expanded =
            self.terminal
                .within_session(&environment.entry(), entry, |mut session| {
                    let source = session.add_source(Self::TERM_SOURCE, term);
                    ExpandedPage::expand(&mut session, &read, source)
                })?;
        expanded.text()
    }

    fn describe_with_type(
        &mut self,
        environment: &DocEnvironment,
        term: &str,
    ) -> Result<DocOutput, ExitStatus> {
        let read = self.read_term(term)?;
        let entry = environment.load(self.terminal)?;
        let lookup = self
            .terminal
            .within_session(&environment.entry(), entry, |session| {
                TypedLookup::new(session, (Self::TERM_SOURCE, term)).look_up(&read)
            })?;
        let page = lookup.page.text()?;
        let type_line = self
            .terminal
            .check_expanded(&lookup.program)
            .map(|typed| lookup.type_line(&typed));
        Ok(DocOutput::typed(page, type_line))
    }

    fn read_term(&self, term: &str) -> Result<Form, ExitStatus> {
        let forms = self
            .terminal
            .scratch()
            .read_text(Self::TERM_SOURCE, term)
            .map_err(|_| ExitStatus::UsageError)?;
        match forms.as_slice() {
            [form] => Ok(form.clone()),
            _ => {
                StandardError::write_line(format_args!("error: `{term}` is not one term"));
                Err(ExitStatus::UsageError)
            }
        }
    }

    fn lookup_call(lookup: &str, term: &Form, source: SourceId) -> Form {
        let span = Span::new(source, term.span().range());
        let head = Form::new(FormKind::Symbol(Symbol::new(lookup)), span);
        Form::new(
            FormKind::List(vec![head, Self::relocated(term, source)]),
            span,
        )
    }

    fn relocated(form: &Form, source: SourceId) -> Form {
        let kind = match form.kind() {
            FormKind::List(items) => FormKind::List(
                items
                    .iter()
                    .map(|item| Self::relocated(item, source))
                    .collect(),
            ),
            atom => atom.clone(),
        };
        Form::new(kind, Span::new(source, form.span().range()))
    }
}

struct ExpandedPage {
    header: Expr,
    body: Expr,
}

impl ExpandedPage {
    fn expand(
        session: &mut ExpansionSession<'_>,
        term: &Form,
        source: SourceId,
    ) -> Result<Self, Diagnostics> {
        let header = session.expression(DocLookup::lookup_call(
            DocLookup::HEADER_MACRO,
            term,
            source,
        ))?;
        let body =
            session.expression(DocLookup::lookup_call(DocLookup::BODY_MACRO, term, source))?;
        Ok(Self { header, body })
    }

    fn text(&self) -> Result<PageText, ExitStatus> {
        Ok(PageText {
            header: Self::string(&self.header, DocLookup::HEADER_MACRO)?,
            body: Self::string(&self.body, DocLookup::BODY_MACRO)?,
        })
    }

    fn same_text(&self, other: &Self) -> bool {
        self.literals() == other.literals()
    }

    fn literals(&self) -> [Option<&str>; 2] {
        [Self::literal(&self.header), Self::literal(&self.body)]
    }

    fn string(expanded: &Expr, lookup: &str) -> Result<String, ExitStatus> {
        match Self::literal(expanded) {
            Some(text) => Ok(text.to_string()),
            None => {
                StandardError::write_line(format_args!(
                    "error: `{lookup}` did not expand to a string"
                ));
                Err(ExitStatus::ProgramErrors)
            }
        }
    }

    fn literal(expanded: &Expr) -> Option<&str> {
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
        let mut program = self.session.finish()?;
        if let Some(binding) = signed {
            program.check_also(binding);
        }
        Ok(TypedPage {
            page,
            signed,
            program,
        })
    }

    fn signed_binding(
        &mut self,
        term: &Form,
        page: &ExpandedPage,
    ) -> Result<Option<BindingId>, Diagnostics> {
        let binding = self.session.binding(term);
        let Some(value_term) = self.session.hide_type(term) else {
            return Ok(binding);
        };
        let value_page = self.expand_page(&value_term)?;
        Ok(if value_page.same_text(page) {
            binding
        } else {
            None
        })
    }

    fn expand_page(&mut self, term: &Form) -> Result<ExpandedPage, Diagnostics> {
        ExpandedPage::expand(&mut self.session, term, self.source)
    }
}

struct TypedPage {
    page: ExpandedPage,
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

#[cfg(test)]
mod tests {
    use super::{DocOutput, PageText};
    use crate::cli::exit_status::ExitStatus;
    use crate::cli::token_colors::TokenColors;

    #[test]
    fn the_type_line_and_the_header_take_the_token_colors_and_the_body_stays_plain() {
        let output = DocOutput {
            type_line: Some("(type x :k)".to_string()),
            page: PageText {
                header: "(defn x (:k))".to_string(),
                body: "\n\nTakes :k 2.".to_string(),
            },
            status: ExitStatus::Success,
        };
        let colors = TokenColors::Painted;
        assert_eq!(
            output.text(colors),
            format!(
                "{}\n{}\n\nTakes :k 2.\n",
                colors.paint("(type x :k)"),
                colors.paint("(defn x (:k))")
            )
        );
    }
}
