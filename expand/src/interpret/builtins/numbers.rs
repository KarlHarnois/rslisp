use std::cmp::Ordering;

use crate::diagnostic::Diagnostic;
use crate::interpret::Value;
use crate::interpret::arguments::Arguments;
use crate::interpret::problems::InterpretProblems;
use crate::reader::{Form, FormKind, Number, NumberValue};
use crate::source::Span;

pub struct NumberOperations<'a> {
    arguments: &'a Arguments,
    call_site: Span,
}

impl<'a> NumberOperations<'a> {
    pub fn new(arguments: &'a Arguments, call_site: Span) -> Self {
        Self {
            arguments,
            call_site,
        }
    }

    pub fn add(&self) -> Result<Value, Diagnostic> {
        self.arithmetic(i128::checked_add)
    }

    pub fn subtract(&self) -> Result<Value, Diagnostic> {
        self.arithmetic(i128::checked_sub)
    }

    pub fn compare(&self, accepts: fn(Ordering) -> bool) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let ordering = self.arguments.integer(0)?.cmp(&self.arguments.integer(1)?);
        Ok(Value::Bool(accepts(ordering)))
    }

    fn arithmetic(&self, operation: fn(i128, i128) -> Option<i128>) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let result =
            operation(self.arguments.integer(0)?, self.arguments.integer(1)?).ok_or_else(|| {
                InterpretProblems::arithmetic_failed(
                    self.arguments.function(),
                    self.arguments.span(),
                )
            })?;
        Ok(self.integer_form(result))
    }

    fn integer_form(&self, value: i128) -> Value {
        let number = Number::new(value.to_string(), NumberValue::Integer(value));
        Value::Form(Form::new(FormKind::Number(number), self.call_site))
    }
}
