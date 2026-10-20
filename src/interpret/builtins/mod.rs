mod lists;
mod numbers;
mod text;

use super::arguments::Arguments;
use super::problems::InterpretProblems;
use super::{CompileTimeHost, Value};
use crate::core::Builtin;
use crate::diagnostic::Diagnostic;
use crate::reader::{Form, FormKind};
use crate::source::Span;
use lists::ListOperations;
use numbers::NumberOperations;
use text::TextOperations;

pub struct BuiltinCall {
    builtin: Builtin,
    arguments: Arguments,
    call_site: Span,
}

impl BuiltinCall {
    pub fn new(builtin: Builtin, values: Vec<Value>, span: Span, call_site: Span) -> Self {
        Self {
            builtin,
            arguments: Arguments::new(builtin.name(), values, span),
            call_site,
        }
    }

    pub fn constant(builtin: Builtin) -> Option<Value> {
        match builtin {
            Builtin::BoolTrue => Some(Value::Bool(true)),
            Builtin::BoolFalse => Some(Value::Bool(false)),
            _ => None,
        }
    }

    pub fn run(self, host: &mut dyn CompileTimeHost) -> Result<Value, Diagnostic> {
        let lists = ListOperations::new(&self.arguments, self.call_site);
        let numbers = NumberOperations::new(&self.arguments, self.call_site);
        let text = TextOperations::new(&self.arguments, self.call_site);
        match self.builtin {
            Builtin::First => lists.first(),
            Builtin::Rest => lists.rest(),
            Builtin::List => lists.list(),
            Builtin::Prepend => lists.prepend(),
            Builtin::Concat => lists.concat(),
            Builtin::Length => lists.length(),
            Builtin::IsEmpty => lists.is_empty(),
            Builtin::Nth => lists.nth(),
            Builtin::Take => lists.take(),
            Builtin::Drop => lists.drop(),
            Builtin::IsSymbol => text.is_kind(|kind| matches!(kind, FormKind::Symbol(_))),
            Builtin::IsList => text.is_kind(|kind| matches!(kind, FormKind::List(_))),
            Builtin::IsString => text.is_kind(|kind| matches!(kind, FormKind::String(_))),
            Builtin::IsNumber => text.is_kind(|kind| matches!(kind, FormKind::Number(_))),
            Builtin::SymbolName => text.symbol_name(),
            Builtin::MakeSymbol => text.make_symbol(),
            Builtin::MakeString => text.make_string(),
            Builtin::SameBinding => text.same_binding(host),
            Builtin::Fail => text.fail(),
            Builtin::ValueEq => text.value_eq(),
            Builtin::BoolNot => text.not(),
            Builtin::NumberAdd => numbers.add(),
            Builtin::NumberSub => numbers.subtract(),
            Builtin::NumberMul => numbers.multiply(),
            Builtin::NumberDiv => numbers.divide(),
            Builtin::NumberRem => numbers.remainder(),
            Builtin::NumberLt => numbers.compare(|order| order.is_lt()),
            Builtin::NumberLe => numbers.compare(|order| order.is_le()),
            Builtin::NumberGt => numbers.compare(|order| order.is_gt()),
            Builtin::NumberGe => numbers.compare(|order| order.is_ge()),
            Builtin::BoolTrue | Builtin::BoolFalse => {
                let constant = Value::Bool(self.builtin == Builtin::BoolTrue);
                Err(InterpretProblems::not_callable(
                    &constant,
                    self.arguments.span(),
                ))
            }
        }
    }
}

struct Forms;

impl Forms {
    fn list(items: Vec<Form>, span: Span) -> Value {
        Value::Form(Form::new(FormKind::List(items), span))
    }
}
