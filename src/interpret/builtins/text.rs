use crate::diagnostic::Diagnostic;
use crate::interpret::arguments::Arguments;
use crate::interpret::problems::InterpretProblems;
use crate::interpret::{CompileTimeHost, Value};
use crate::reader::{Form, FormKind, Symbol};
use crate::source::Span;

pub struct TextOperations<'a> {
    arguments: &'a Arguments,
    call_site: Span,
}

impl<'a> TextOperations<'a> {
    const FAIL_ARGUMENTS: usize = 2;

    pub fn new(arguments: &'a Arguments, call_site: Span) -> Self {
        Self {
            arguments,
            call_site,
        }
    }

    pub fn is_kind(&self, matches: fn(&FormKind) -> bool) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        let is_match = self
            .arguments
            .value(0)
            .as_form()
            .is_some_and(|form| matches(form.kind()));
        Ok(Value::Bool(is_match))
    }

    pub fn symbol_name(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        let name = self.arguments.symbol(0)?.name().to_string();
        Ok(self.string_value(name))
    }

    pub fn make_symbol(&self) -> Result<Value, Diagnostic> {
        let context = self.arguments.symbol(0)?;
        let name = self.joined_parts(1)?;
        let symbol = Symbol::with_scopes(name, context.scopes().clone());
        Ok(Value::Form(Form::new(
            FormKind::Symbol(symbol),
            self.call_site,
        )))
    }

    pub fn make_string(&self) -> Result<Value, Diagnostic> {
        Ok(self.string_value(self.joined_parts(0)?))
    }

    pub fn same_binding(&self, host: &mut dyn CompileTimeHost) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let left = self.arguments.symbol(0)?;
        let right = self.arguments.symbol(1)?;
        Ok(Value::Bool(host.same_binding(left, right)))
    }

    pub fn fail(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(Self::FAIL_ARGUMENTS)?;
        let message = self.arguments.string(0)?.to_string();
        let culprit = self.arguments.form(1)?;
        Err(InterpretProblems::failed(message, culprit.span()))
    }

    pub fn value_eq(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let equal = match (self.arguments.value(0), self.arguments.value(1)) {
            (Value::Form(left), Value::Form(right)) => Self::forms_equal(left, right),
            (Value::Bool(left), Value::Bool(right)) => left == right,
            (Value::Unit, Value::Unit) => true,
            _ => false,
        };
        Ok(Value::Bool(equal))
    }

    pub fn not(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        Ok(Value::Bool(!self.arguments.boolean(0)?))
    }

    fn forms_equal(left: &Form, right: &Form) -> bool {
        match (left.kind(), right.kind()) {
            (FormKind::Symbol(left), FormKind::Symbol(right)) => left.name() == right.name(),
            (FormKind::String(left), FormKind::String(right)) => left == right,
            (FormKind::Number(left), FormKind::Number(right)) => left.value() == right.value(),
            (FormKind::List(left), FormKind::List(right)) => {
                left.len() == right.len()
                    && left
                        .iter()
                        .zip(right)
                        .all(|(left, right)| Self::forms_equal(left, right))
            }
            _ => false,
        }
    }

    fn joined_parts(&self, start: usize) -> Result<String, Diagnostic> {
        (start..self.arguments.values().len())
            .map(|index| self.part_text(index))
            .collect()
    }

    fn part_text(&self, index: usize) -> Result<String, Diagnostic> {
        let form = self.arguments.form(index)?;
        Ok(match form.kind() {
            FormKind::Symbol(symbol) => symbol.name().to_string(),
            FormKind::String(text) => text.to_string(),
            FormKind::Number(number) => number.text().to_string(),
            FormKind::List(_) => form.to_string(),
        })
    }

    fn string_value(&self, text: String) -> Value {
        Value::Form(Form::new(FormKind::String(text.into()), self.call_site))
    }
}
