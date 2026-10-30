use super::problems::InterpretProblems;
use super::{CodeList, Value};
use crate::diagnostic::Diagnostic;
use crate::reader::{Form, FormKind, NumberValue, Symbol};
use crate::source::Span;

pub struct Arguments {
    function: &'static str,
    values: Vec<Value>,
    span: Span,
}

impl Arguments {
    pub fn new(function: &'static str, values: Vec<Value>, span: Span) -> Self {
        Self {
            function,
            values,
            span,
        }
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn function(&self) -> &'static str {
        self.function
    }

    pub fn values(&self) -> &[Value] {
        &self.values
    }

    pub fn expect_count(&self, expected: usize) -> Result<(), Diagnostic> {
        if self.values.len() == expected {
            Ok(())
        } else {
            Err(InterpretProblems::builtin_argument_count(
                self.function,
                expected,
                self.values.len(),
                self.span,
            ))
        }
    }

    pub fn value(&self, index: usize) -> &Value {
        &self.values[index]
    }

    pub fn code(&self, index: usize) -> Result<Value, Diagnostic> {
        let value = self.value(index);
        if value.is_code() {
            Ok(value.clone())
        } else {
            Err(self.wrong(index, "code"))
        }
    }

    pub fn form(&self, index: usize) -> Result<Form, Diagnostic> {
        self.values[index]
            .to_form()
            .ok_or_else(|| self.wrong(index, "code"))
    }

    pub fn list(&self, index: usize) -> Result<CodeList, Diagnostic> {
        self.code(index)?
            .as_code_list()
            .ok_or_else(|| self.wrong(index, "a list"))
    }

    pub fn list_forms(&self, index: usize) -> Result<Vec<Form>, Diagnostic> {
        Ok(self.list(index)?.forms())
    }

    pub fn symbol(&self, index: usize) -> Result<&Symbol, Diagnostic> {
        self.atom(index, "a symbol")?
            .as_symbol()
            .ok_or_else(|| self.wrong(index, "a symbol"))
    }

    pub fn symbol_forms(&self, index: usize) -> Result<Vec<Form>, Diagnostic> {
        let forms = self.list_forms(index)?;
        if forms.iter().all(|form| form.as_symbol().is_some()) {
            Ok(forms)
        } else {
            Err(self.wrong(index, "a list of symbols"))
        }
    }

    pub fn keyword_name(&self, index: usize) -> Result<&str, Diagnostic> {
        self.values[index]
            .keyword_name()
            .ok_or_else(|| self.wrong(index, "a keyword"))
    }

    pub fn keyword_field(&self, index: usize) -> Result<Form, Diagnostic> {
        self.values[index]
            .keyword_field()
            .ok_or_else(|| self.wrong(index, "a keyword"))
    }

    pub fn field_keyword(&self, index: usize) -> Result<Form, Diagnostic> {
        self.values[index]
            .field_keyword()
            .ok_or_else(|| self.wrong(index, "a field name"))
    }

    pub fn string(&self, index: usize) -> Result<&str, Diagnostic> {
        match self.atom(index, "a string")?.kind() {
            FormKind::String(text) => Ok(text),
            _ => Err(self.wrong(index, "a string")),
        }
    }

    pub fn integer(&self, index: usize) -> Result<i128, Diagnostic> {
        match self.atom(index, "an integer")?.kind() {
            FormKind::Number(number) => match number.value() {
                NumberValue::Integer(value) => Ok(*value),
                NumberValue::Float(_) => Err(self.wrong(index, "an integer")),
            },
            _ => Err(self.wrong(index, "an integer")),
        }
    }

    pub fn index(&self, index: usize) -> Result<usize, Diagnostic> {
        usize::try_from(self.integer(index)?).map_err(|_| self.wrong(index, "a count"))
    }

    pub fn boolean(&self, index: usize) -> Result<bool, Diagnostic> {
        match self.values[index] {
            Value::Bool(value) => Ok(value),
            _ => Err(self.wrong(index, "a boolean")),
        }
    }

    fn atom(&self, index: usize, expected: &str) -> Result<&Form, Diagnostic> {
        match &self.values[index] {
            Value::Form(form) => Ok(form),
            Value::List(_) => Err(self.wrong(index, expected)),
            _ => Err(self.wrong(index, "code")),
        }
    }

    fn wrong(&self, index: usize, expected: &str) -> Diagnostic {
        let position = index + 1;
        InterpretProblems::wrong_argument(
            self.function,
            position,
            expected,
            &self.values[index],
            self.span,
        )
    }
}
