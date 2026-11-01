use std::rc::Rc;

use super::CodeList;
use crate::core::{BindingId, Builtin, Keyword, Lambda};
use crate::reader::{Form, FormKind, NumberValue, Symbol};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Value {
    Form(Form),
    List(CodeList),
    Bool(bool),
    Unit,
    Closure(Rc<Closure>),
    Builtin(Builtin),
}

impl Value {
    pub fn describe(&self) -> &'static str {
        match self {
            Value::Form(form) => Self::describe_form(form),
            Value::List(_) => "a list",
            Value::Bool(_) => "a boolean",
            Value::Unit => "unit",
            Value::Closure(_) | Value::Builtin(_) => "a function",
        }
    }

    fn describe_form(form: &Form) -> &'static str {
        match form.kind() {
            FormKind::Symbol(symbol) if Keyword::field_name(symbol.name()).is_some() => "a keyword",
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

    pub fn to_form(&self) -> Option<Form> {
        match self {
            Value::Form(form) => Some(form.clone()),
            Value::List(list) => Some(list.to_form()),
            _ => None,
        }
    }

    pub fn to_form_within(&self, allowance: &mut usize) -> Option<Form> {
        match self {
            Value::List(list) => list.to_form_within(allowance),
            _ => {
                *allowance = allowance.checked_sub(1)?;
                self.to_form()
            }
        }
    }

    pub fn is_code(&self) -> bool {
        matches!(self, Value::Form(_) | Value::List(_))
    }

    pub fn is_list(&self) -> bool {
        self.as_code_list().is_some()
    }

    pub fn as_code_list(&self) -> Option<CodeList> {
        match self {
            Value::Form(form) => CodeList::of_form(form),
            Value::List(list) => Some(list.clone()),
            _ => None,
        }
    }

    pub fn keyword_name(&self) -> Option<&str> {
        self.as_form()
            .and_then(Form::as_symbol)
            .and_then(|symbol| Keyword::field_name(symbol.name()))
    }

    pub fn keyword_field(&self) -> Option<Form> {
        let form = self.as_form()?;
        let keyword = form.as_symbol()?;
        let field_name = Keyword::field_name(keyword.name())?;
        let field = Symbol::with_scopes(field_name, keyword.scopes().clone());
        Some(Form::new(FormKind::Symbol(field), form.span()))
    }

    pub fn field_keyword(&self) -> Option<Form> {
        let form = self.as_form()?;
        let field = form.as_symbol()?;
        let keyword_name = Keyword::of_field(field.name())?;
        let keyword = Symbol::with_scopes(keyword_name, field.scopes().clone());
        Some(Form::new(FormKind::Symbol(keyword), form.span()))
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Closure {
    pub lambda: Rc<Lambda>,
    pub env: Env,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Env(Option<Rc<Frame>>);

#[derive(Debug, serde::Serialize, serde::Deserialize)]
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
