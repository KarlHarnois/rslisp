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
        self.arithmetic(i128::checked_add, |left, right| left + right)
    }

    pub fn subtract(&self) -> Result<Value, Diagnostic> {
        self.arithmetic(i128::checked_sub, |left, right| left - right)
    }

    pub fn multiply(&self) -> Result<Value, Diagnostic> {
        self.arithmetic(i128::checked_mul, |left, right| left * right)
    }

    pub fn divide(&self) -> Result<Value, Diagnostic> {
        self.arithmetic(i128::checked_div, |left, right| left / right)
    }

    pub fn remainder(&self) -> Result<Value, Diagnostic> {
        self.arithmetic(i128::checked_rem, |left, right| left % right)
    }

    pub fn compare(&self, accepts: fn(Ordering) -> bool) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let ordering = match (self.arguments.number(0)?, self.arguments.number(1)?) {
            (NumberValue::Integer(left), NumberValue::Integer(right)) => Some(left.cmp(right)),
            (left, right) => Self::as_float(left).partial_cmp(&Self::as_float(right)),
        };
        Ok(Value::Bool(ordering.is_some_and(accepts)))
    }

    fn arithmetic(
        &self,
        integer: fn(i128, i128) -> Option<i128>,
        float: fn(f64, f64) -> f64,
    ) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let result = match (self.arguments.number(0)?, self.arguments.number(1)?) {
            (NumberValue::Integer(left), NumberValue::Integer(right)) => {
                integer(*left, *right).map(NumberValue::Integer)
            }
            (left, right) => Some(float(Self::as_float(left), Self::as_float(right)))
                .filter(|value| value.is_finite())
                .map(NumberValue::Float),
        };
        let value = result.ok_or_else(|| {
            InterpretProblems::arithmetic_failed(self.arguments.function(), self.arguments.span())
        })?;
        Ok(self.number_form(value))
    }

    fn number_form(&self, value: NumberValue) -> Value {
        let text = match &value {
            NumberValue::Integer(integer) => integer.to_string(),
            NumberValue::Float(float) => format!("{float:?}"),
        };
        Value::Form(Form::new(
            FormKind::Number(Number::new(text, value)),
            self.call_site,
        ))
    }

    fn as_float(value: &NumberValue) -> f64 {
        match value {
            NumberValue::Integer(integer) => *integer as f64,
            NumberValue::Float(float) => *float,
        }
    }
}
