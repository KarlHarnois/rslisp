mod cursor;
mod form;
mod number;
mod problems;
mod string_literal;
mod syntax;

pub use form::{Form, FormKind, Number, NumberValue, Symbol};

use std::ops::Range;

use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::source::{SourceFile, Span};
use cursor::Cursor;
use number::{NumberError, NumberLiteral};
use problems::ReadProblems;
use string_literal::{StringLiteralReader, StringProblem};
use syntax::Syntax;

pub struct Reader<'a> {
    source: &'a SourceFile,
    cursor: Cursor<'a>,
    diagnostics: Diagnostics,
    string_ran_to_end: bool,
    open_lists: usize,
}

impl<'a> Reader<'a> {
    pub fn new(source: &'a SourceFile) -> Self {
        Self {
            source,
            cursor: Cursor::new(source.text()),
            diagnostics: Diagnostics::default(),
            string_ran_to_end: false,
            open_lists: 0,
        }
    }

    pub fn read(mut self) -> Result<Vec<Form>, Diagnostics> {
        let forms = self.read_top_level();
        self.diagnostics.into_result(forms)
    }

    fn read_top_level(&mut self) -> Vec<Form> {
        let mut forms = Vec::new();
        while let Some(next) = self.skip_trivia() {
            if next == Syntax::CLOSE {
                self.skip_unmatched_close();
            } else {
                forms.extend(self.read_form());
            }
        }
        forms
    }

    fn skip_trivia(&mut self) -> Option<char> {
        loop {
            self.cursor.advance_while(char::is_whitespace);
            if self.cursor.peek()? != Syntax::COMMENT {
                return self.cursor.peek();
            }
            self.cursor
                .advance_while(|character| character != Syntax::NEWLINE);
        }
    }

    fn read_form(&mut self) -> Option<Form> {
        let next = self.cursor.peek()?;
        if next == Syntax::OPEN {
            self.read_list()
        } else if next == Syntax::QUOTE {
            Some(self.read_string())
        } else if Syntax::is_reserved(next) {
            self.skip_reserved(next);
            None
        } else {
            self.read_atom()
        }
    }

    fn read_list(&mut self) -> Option<Form> {
        if self.open_lists == Form::NESTING_LIMIT {
            self.skip_list_past_limit();
            return None;
        }
        let start = self.cursor.position();
        self.cursor.advance();
        self.open_lists += 1;
        let items = self.read_list_items(start);
        self.open_lists -= 1;
        Some(Form::new(FormKind::List(items), self.span_from(start)))
    }

    fn skip_list_past_limit(&mut self) {
        let start = self.cursor.position();
        let open = self.span_of_char(start, Syntax::OPEN);
        self.report(ReadProblems::list_too_deep(open, Form::NESTING_LIMIT));
        self.skip_list(start);
    }

    fn skip_list(&mut self, start: usize) {
        let mut unclosed = 0;
        while let Some(next) = self.skip_trivia() {
            unclosed = self.skip_list_part(next, unclosed);
            if unclosed == 0 {
                return;
            }
        }
        self.report_unclosed_list(start);
    }

    fn skip_list_part(&mut self, next: char, unclosed: usize) -> usize {
        match next {
            Syntax::OPEN => {
                self.cursor.advance();
                unclosed + 1
            }
            Syntax::CLOSE => {
                self.cursor.advance();
                unclosed - 1
            }
            _ => {
                self.read_form();
                unclosed
            }
        }
    }

    fn read_list_items(&mut self, start: usize) -> Vec<Form> {
        let mut items = Vec::new();
        loop {
            match self.skip_trivia() {
                None => return self.unclosed_list(start, items),
                Some(Syntax::CLOSE) => return self.closed_list(items),
                Some(_) => items.extend(self.read_form()),
            }
        }
    }

    fn closed_list(&mut self, items: Vec<Form>) -> Vec<Form> {
        self.cursor.advance();
        items
    }

    fn unclosed_list(&mut self, start: usize, items: Vec<Form>) -> Vec<Form> {
        self.report_unclosed_list(start);
        items
    }

    fn report_unclosed_list(&mut self, start: usize) {
        if !self.string_ran_to_end {
            let open = self.span_of_char(start, Syntax::OPEN);
            self.report(ReadProblems::unclosed_list(open));
        }
    }

    fn read_string(&mut self) -> Form {
        let start = self.cursor.position();
        let literal = StringLiteralReader::read(&mut self.cursor);
        for problem in literal.problems {
            self.report_string_problem(problem, start);
        }
        Form::new(
            FormKind::String(literal.value.into()),
            self.span_from(start),
        )
    }

    fn report_string_problem(&mut self, problem: StringProblem, start: usize) {
        let diagnostic = match problem {
            StringProblem::Unterminated => {
                self.string_ran_to_end = true;
                ReadProblems::unterminated_string(self.span_of_char(start, Syntax::QUOTE))
            }
            StringProblem::UnknownEscape(range) => ReadProblems::unknown_escape(self.span(range)),
        };
        self.report(diagnostic);
    }

    fn read_atom(&mut self) -> Option<Form> {
        let start = self.cursor.position();
        let text = self
            .cursor
            .advance_while(|character| !Syntax::ends_atom(character));
        let span = self.span_from(start);
        let literal = NumberLiteral::new(text);
        if !literal.is_number_shaped() {
            return Some(Form::new(FormKind::Symbol(Symbol::new(text)), span));
        }
        match literal.parse() {
            Ok(number) => Some(Form::new(FormKind::Number(number), span)),
            Err(error) => self.skip_invalid_number(error, text, span),
        }
    }

    fn skip_invalid_number(&mut self, error: NumberError, text: &str, span: Span) -> Option<Form> {
        let diagnostic = match error {
            NumberError::Malformed => ReadProblems::malformed_number(text, span),
            NumberError::TooLarge => ReadProblems::number_too_large(text, span),
        };
        self.report(diagnostic);
        None
    }

    fn skip_unmatched_close(&mut self) {
        let close = self.span_of_char(self.cursor.position(), Syntax::CLOSE);
        self.cursor.advance();
        self.report(ReadProblems::unmatched_close(close));
    }

    fn skip_reserved(&mut self, character: char) {
        let span = self.span_of_char(self.cursor.position(), character);
        self.cursor.advance();
        self.report(ReadProblems::reserved_character(character, span));
    }

    fn report(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    fn span_from(&self, start: usize) -> Span {
        self.span(start..self.cursor.position())
    }

    fn span_of_char(&self, start: usize, character: char) -> Span {
        self.span(start..start + character.len_utf8())
    }

    fn span(&self, range: Range<usize>) -> Span {
        Span::new(self.source.id(), range)
    }
}
