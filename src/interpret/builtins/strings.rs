use super::Forms;
use crate::diagnostic::Diagnostic;
use crate::interpret::arguments::Arguments;
use crate::interpret::problems::InterpretProblems;
use crate::interpret::{CodeList, Value};
use crate::reader::{Form, FormKind};
use crate::source::Span;

pub struct StringOperations<'a> {
    arguments: &'a Arguments,
    call_site: Span,
}

impl<'a> StringOperations<'a> {
    const LINE_BREAK: char = '\n';

    pub fn new(arguments: &'a Arguments, call_site: Span) -> Self {
        Self {
            arguments,
            call_site,
        }
    }

    pub fn lines(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        let lines = self
            .arguments
            .string(0)?
            .split(Self::LINE_BREAK)
            .map(|line| self.string_value(line.to_string()))
            .collect();
        Ok(Value::List(CodeList::new(lines, self.call_site)))
    }

    pub fn trim_start(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        let text = self.arguments.string(0)?;
        Ok(self.string_value(text.trim_start().to_string()))
    }

    pub fn length(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        let count = self.arguments.string(0)?.chars().count();
        Ok(Forms::count(count, self.call_site))
    }

    pub fn drop(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let count = self.arguments.index(0)?;
        let text = self.arguments.string(1)?;
        let start = self.char_boundary(text, count)?;
        Ok(self.string_value(text[start..].to_string()))
    }

    fn char_boundary(&self, text: &str, count: usize) -> Result<usize, Diagnostic> {
        text.char_indices()
            .map(|(boundary, _)| boundary)
            .chain(std::iter::once(text.len()))
            .nth(count)
            .ok_or_else(|| {
                InterpretProblems::past_string_end(self.arguments.function(), self.arguments.span())
            })
    }

    fn string_value(&self, text: String) -> Value {
        Value::Form(Form::new(FormKind::String(text.into()), self.call_site))
    }
}
