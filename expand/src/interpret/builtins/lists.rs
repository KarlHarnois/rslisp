use std::ops::Range;

use super::Forms;
use crate::diagnostic::Diagnostic;
use crate::interpret::arguments::Arguments;
use crate::interpret::problems::InterpretProblems;
use crate::interpret::{CodeList, Value};
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
        self.item(&self.arguments.list(0)?, 0)
    }

    pub fn rest(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        self.dropped(&self.arguments.list(0)?, 1)
    }

    pub fn list(&self) -> Result<Value, Diagnostic> {
        let values = (0..self.arguments.values().len())
            .map(|index| self.arguments.code(index))
            .collect::<Result<Vec<Value>, Diagnostic>>()?;
        Ok(Value::List(CodeList::new(values, self.call_site)))
    }

    pub fn prepend(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let item = self.arguments.code(0)?;
        let items = self.arguments.list(1)?;
        Ok(Value::List(items.prepended(item, self.call_site)))
    }

    pub fn concat(&self) -> Result<Value, Diagnostic> {
        let lists = (0..self.arguments.values().len())
            .map(|index| self.arguments.list(index))
            .collect::<Result<Vec<CodeList>, Diagnostic>>()?;
        let Some((head, tail)) = lists.split_first() else {
            return Ok(Value::List(CodeList::new(Vec::new(), self.call_site)));
        };
        let extra = tail.iter().flat_map(CodeList::values).collect();
        Ok(Value::List(head.appended(extra, self.call_site)))
    }

    pub fn length(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        Ok(Forms::count(self.arguments.list(0)?.len(), self.call_site))
    }

    pub fn is_empty(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        Ok(Value::Bool(self.arguments.list(0)?.is_empty()))
    }

    pub fn nth(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let items = self.arguments.list(0)?;
        self.item(&items, self.arguments.index(1)?)
    }

    pub fn take(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let count = self.arguments.index(0)?;
        let items = self.arguments.list(1)?;
        self.sliced(&items, 0..count)
    }

    pub fn drop(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let count = self.arguments.index(0)?;
        self.dropped(&self.arguments.list(1)?, count)
    }

    fn item(&self, items: &CodeList, index: usize) -> Result<Value, Diagnostic> {
        items.get(index).ok_or_else(|| self.out_of_range())
    }

    fn dropped(&self, items: &CodeList, count: usize) -> Result<Value, Diagnostic> {
        self.sliced(items, count..items.len())
    }

    fn sliced(&self, items: &CodeList, range: Range<usize>) -> Result<Value, Diagnostic> {
        items
            .slice(range, self.call_site)
            .map(Value::List)
            .ok_or_else(|| self.out_of_range())
    }

    fn out_of_range(&self) -> Diagnostic {
        InterpretProblems::out_of_range(self.arguments.function(), self.arguments.span())
    }
}
