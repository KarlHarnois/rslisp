use std::rc::Rc;

use crate::core::{BindingId, Builtin, Keyword, Lambda};
use crate::reader::{Form, FormKind, NumberValue};

#[derive(Clone, Debug)]
pub enum Value {
    Form(Form),
    Bool(bool),
    Unit,
    Closure(Rc<Closure>),
    Builtin(Builtin),
}

impl Value {
    pub fn describe(&self) -> &'static str {
        match self {
            Value::Form(form) => Self::describe_form(form),
            Value::Bool(_) => "a boolean",
            Value::Unit => "unit",
            Value::Closure(_) | Value::Builtin(_) => "a function",
        }
    }

    fn describe_form(form: &Form) -> &'static str {
        match form.kind() {
            FormKind::Symbol(_) => "a symbol",
            FormKind::String(_) => "a string",
            FormKind::Number(number) => match number.value() {
                NumberValue::Integer(_) => "an integer",
                NumberValue::Float(_) => "a float",
            },
            FormKind::List(_) => "a list",
        }
    }

    pub fn as_form(&self) -> Option<&Form> {
        match self {
            Value::Form(form) => Some(form),
            _ => None,
        }
    }

    pub fn keyword_name(&self) -> Option<&str> {
        self.as_form()
            .and_then(Form::as_symbol)
            .and_then(|symbol| Keyword::field_name(symbol.name()))
    }
}

#[derive(Debug)]
pub struct Closure {
    pub lambda: Rc<Lambda>,
    pub env: Env,
}

#[derive(Clone, Debug, Default)]
pub struct Env(Option<Rc<Frame>>);

#[derive(Debug)]
struct Frame {
    binding: BindingId,
    value: Value,
    parent: Env,
}

impl Env {
    pub fn bind(&self, binding: BindingId, value: Value) -> Env {
        Env(Some(Rc::new(Frame {
            binding,
            value,
            parent: self.clone(),
        })))
    }

    pub fn lookup(&self, binding: BindingId) -> Option<&Value> {
        let mut current = self.0.as_deref();
        while let Some(frame) = current {
            if frame.binding == binding {
                return Some(&frame.value);
            }
            current = frame.parent.0.as_deref();
        }
        None
    }
}
