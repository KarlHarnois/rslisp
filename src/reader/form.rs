use std::fmt;
use std::rc::Rc;

use crate::source::Span;

#[derive(Clone, Debug)]
pub struct Form {
    kind: FormKind,
    span: Span,
}

#[derive(Clone, Debug)]
pub enum FormKind {
    Symbol(Symbol),
    String(Rc<str>),
    Number(Number),
    List(Vec<Form>),
}

impl Form {
    pub fn new(kind: FormKind, span: Span) -> Self {
        Self { kind, span }
    }

    pub fn kind(&self) -> &FormKind {
        &self.kind
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn as_symbol(&self) -> Option<&Symbol> {
        match &self.kind {
            FormKind::Symbol(symbol) => Some(symbol),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Form]> {
        match &self.kind {
            FormKind::List(items) => Some(items),
            _ => None,
        }
    }
}

impl fmt::Display for Form {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            FormKind::Symbol(symbol) => write!(formatter, "{}", symbol.name()),
            FormKind::String(text) => write!(formatter, "{text:?}"),
            FormKind::Number(number) => write!(formatter, "{}", number.text()),
            FormKind::List(items) => Self::fmt_list(items, formatter),
        }
    }
}

impl Form {
    fn fmt_list(items: &[Form], formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rendered: Vec<String> = items.iter().map(Form::to_string).collect();
        write!(formatter, "({})", rendered.join(" "))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Symbol {
    name: Rc<str>,
}

impl Symbol {
    pub fn new(name: impl Into<Rc<str>>) -> Self {
        Self { name: name.into() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum NumberValue {
    Integer(i128),
    Float(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Number {
    text: Rc<str>,
    value: NumberValue,
}

impl Number {
    pub fn new(text: impl Into<Rc<str>>, value: NumberValue) -> Self {
        Self {
            text: text.into(),
            value,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn value(&self) -> &NumberValue {
        &self.value
    }
}
