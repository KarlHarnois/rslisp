use super::Forms;
use crate::diagnostic::Diagnostic;
use crate::interpret::Value;
use crate::interpret::arguments::Arguments;
use crate::interpret::problems::InterpretProblems;
use crate::reader::{Form, FormKind, Number, NumberValue};
use crate::source::Span;

pub struct ListOperations<'a> {
    arguments: &'a Arguments,
    call_site: Span,
}

impl<'a> ListOperations<'a> {
    pub fn new(arguments: &'a Arguments, call_site: Span) -> Self {
        Self {
            arguments,
            call_site,
        }
    }

    pub fn first(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        let items = self.arguments.list(0)?;
        items
            .first()
            .map(|item| Value::Form(item.clone()))
            .ok_or_else(|| self.out_of_range())
    }

    pub fn rest(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        let items = self.arguments.list(0)?;
        let remaining = items.get(1..).ok_or_else(|| self.out_of_range())?;
        Ok(self.new_list(remaining.to_vec()))
    }

    pub fn list(&self) -> Result<Value, Diagnostic> {
        let items: Result<Vec<Form>, Diagnostic> = (0..self.arguments.values().len())
            .map(|index| self.arguments.form(index).cloned())
            .collect();
        Ok(self.new_list(items?))
    }

    pub fn prepend(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let mut items = vec![self.arguments.form(0)?.clone()];
        items.extend_from_slice(self.arguments.list(1)?);
        Ok(self.new_list(items))
    }

    pub fn concat(&self) -> Result<Value, Diagnostic> {
        let mut items = Vec::new();
        for index in 0..self.arguments.values().len() {
            items.extend_from_slice(self.arguments.list(index)?);
        }
        Ok(self.new_list(items))
    }

    pub fn length(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        let count = self.arguments.list(0)?.len();
        let number = Number::new(count.to_string(), NumberValue::Integer(count as i128));
        Ok(Value::Form(Form::new(
            FormKind::Number(number),
            self.call_site,
        )))
    }

    pub fn is_empty(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        Ok(Value::Bool(self.arguments.list(0)?.is_empty()))
    }

    pub fn nth(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let items = self.arguments.list(0)?;
        let index = self.arguments.index(1)?;
        items
            .get(index)
            .map(|item| Value::Form(item.clone()))
            .ok_or_else(|| self.out_of_range())
    }

    pub fn take(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let count = self.arguments.index(0)?;
        let items = self.arguments.list(1)?;
        let taken = items.get(..count).ok_or_else(|| self.out_of_range())?;
        Ok(self.new_list(taken.to_vec()))
    }

    pub fn drop(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let count = self.arguments.index(0)?;
        let items = self.arguments.list(1)?;
        let remaining = items.get(count..).ok_or_else(|| self.out_of_range())?;
        Ok(self.new_list(remaining.to_vec()))
    }

    fn new_list(&self, items: Vec<Form>) -> Value {
        Forms::list(items, self.call_site)
    }

    fn out_of_range(&self) -> Diagnostic {
        InterpretProblems::out_of_range(self.arguments.function(), self.arguments.span())
    }
}
