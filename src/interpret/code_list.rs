use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use super::Value;
use crate::reader::{Form, FormKind};
use crate::source::Span;

#[derive(Clone, Debug)]
pub struct CodeList {
    items: ListItems,
    range: Range<usize>,
    span: Span,
}

#[derive(Clone, Debug)]
enum ListItems {
    Read(Form),
    Built(Rc<RefCell<Vec<Value>>>),
}

impl CodeList {
    pub fn new(values: Vec<Value>, span: Span) -> Self {
        Self {
            range: 0..values.len(),
            items: ListItems::Built(Rc::new(RefCell::new(values))),
            span,
        }
    }

    pub fn of_form(form: &Form) -> Option<Self> {
        let length = form.as_list()?.len();
        Some(Self {
            items: ListItems::Read(form.clone()),
            range: 0..length,
            span: form.span(),
        })
    }

    pub fn len(&self) -> usize {
        self.range.len()
    }

    pub fn is_empty(&self) -> bool {
        self.range.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<Value> {
        (index < self.len()).then(|| self.item(self.range.start + index))
    }

    pub fn slice(&self, range: Range<usize>, span: Span) -> Option<Self> {
        (range.start <= range.end && range.end <= self.len()).then(|| Self {
            items: self.items.clone(),
            range: self.range.start + range.start..self.range.start + range.end,
            span,
        })
    }

    pub fn appended(&self, extra: Vec<Value>, span: Span) -> Self {
        match self.growable_buffer() {
            Some(buffer) => Self::grown(buffer, self.range.start, extra, span),
            None => Self::new([self.values(), extra].concat(), span),
        }
    }

    pub fn prepended(&self, value: Value, span: Span) -> Self {
        Self::new([vec![value], self.values()].concat(), span)
    }

    pub fn values(&self) -> Vec<Value> {
        self.range.clone().map(|index| self.item(index)).collect()
    }

    pub fn forms(&self) -> Vec<Form> {
        self.values()
            .iter()
            .map(|value| value.to_form().expect("a code list holds only code"))
            .collect()
    }

    pub fn to_form(&self) -> Form {
        Form::new(FormKind::List(self.forms()), self.span)
    }

    pub fn to_form_within(&self, allowance: &mut usize) -> Option<Form> {
        *allowance = allowance.checked_sub(1)?;
        let mut forms = Vec::with_capacity(self.len());
        for index in self.range.clone() {
            forms.push(self.item(index).to_form_within(allowance)?);
        }
        Some(Form::new(FormKind::List(forms), self.span))
    }

    fn item(&self, index: usize) -> Value {
        match &self.items {
            ListItems::Read(form) => {
                Value::Form(form.as_list().expect("read as a list")[index].clone())
            }
            ListItems::Built(buffer) => buffer.borrow()[index].clone(),
        }
    }

    fn growable_buffer(&self) -> Option<&Rc<RefCell<Vec<Value>>>> {
        match &self.items {
            ListItems::Built(buffer) if buffer.borrow().len() == self.range.end => Some(buffer),
            _ => None,
        }
    }

    fn grown(
        buffer: &Rc<RefCell<Vec<Value>>>,
        start: usize,
        extra: Vec<Value>,
        span: Span,
    ) -> Self {
        buffer.borrow_mut().extend(extra);
        Self {
            range: start..buffer.borrow().len(),
            items: ListItems::Built(Rc::clone(buffer)),
            span,
        }
    }
}
