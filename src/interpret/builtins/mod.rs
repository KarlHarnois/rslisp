mod failure;
mod lists;
mod numbers;
mod strings;
mod text;

pub use failure::FailureDescription;

use std::cmp::Ordering;

use super::arguments::Arguments;
use super::{CompileTimeHost, Value};
use crate::core::Builtin;
use crate::diagnostic::Diagnostic;
use crate::reader::{Form, FormKind, Number, NumberValue};
use crate::source::Span;
use lists::ListOperations;
use numbers::NumberOperations;
use strings::StringOperations;
use text::TextOperations;

pub struct BuiltinCall {
    builtin: Builtin,
    arguments: Arguments,
    call_site: Span,
}

impl BuiltinCall {
    const IMPLEMENTATIONS: [(Builtin, Implementation); 43] = [
        (Builtin::First, |operations, _| operations.lists.first()),
        (Builtin::Rest, |operations, _| operations.lists.rest()),
        (Builtin::List, |operations, _| operations.lists.list()),
        (Builtin::Prepend, |operations, _| operations.lists.prepend()),
        (Builtin::Concat, |operations, _| operations.lists.concat()),
        (Builtin::Length, |operations, _| operations.lists.length()),
        (Builtin::IsEmpty, |operations, _| {
            operations.lists.is_empty()
        }),
        (Builtin::Nth, |operations, _| operations.lists.nth()),
        (Builtin::Take, |operations, _| operations.lists.take()),
        (Builtin::Drop, |operations, _| operations.lists.drop()),
        (Builtin::IsSymbol, |operations, _| {
            operations
                .text
                .is_kind(|kind| matches!(kind, FormKind::Symbol(_)))
        }),
        (Builtin::IsKeyword, |operations, _| {
            operations.text.is_keyword()
        }),
        (Builtin::IsList, |operations, _| operations.text.is_list()),
        (Builtin::IsString, |operations, _| {
            operations
                .text
                .is_kind(|kind| matches!(kind, FormKind::String(_)))
        }),
        (Builtin::IsNumber, |operations, _| {
            operations
                .text
                .is_kind(|kind| matches!(kind, FormKind::Number(_)))
        }),
        (Builtin::SymbolName, |operations, _| {
            operations.text.symbol_name()
        }),
        (Builtin::KeywordName, |operations, _| {
            operations.text.keyword_name()
        }),
        (Builtin::KeywordField, |operations, _| {
            operations.text.keyword_field()
        }),
        (Builtin::FieldKeyword, |operations, _| {
            operations.text.field_keyword()
        }),
        (Builtin::MakeSymbol, |operations, _| {
            operations.text.make_symbol()
        }),
        (Builtin::MakeString, |operations, _| {
            operations.text.make_string()
        }),
        (Builtin::SameBinding, |operations, host| {
            operations.text.same_binding(host)
        }),
        (Builtin::DefinitionSymbol, |operations, host| {
            operations.text.definition_symbol(host)
        }),
        (Builtin::IsBound, |operations, host| {
            operations.text.is_bound(host)
        }),
        (Builtin::Fail, |operations, _| operations.text.fail()),
        (Builtin::ClosestName, |operations, _| {
            operations.text.closest_name()
        }),
        (Builtin::ValueEq, |operations, _| operations.text.value_eq()),
        (Builtin::BoolNot, |operations, _| operations.text.not()),
        (Builtin::NumberAdd, |operations, _| operations.numbers.add()),
        (Builtin::NumberSub, |operations, _| {
            operations.numbers.subtract()
        }),
        (Builtin::NumberLt, |operations, _| {
            operations.numbers.compare(Ordering::is_lt)
        }),
        (Builtin::NumberLe, |operations, _| {
            operations.numbers.compare(Ordering::is_le)
        }),
        (Builtin::NumberGt, |operations, _| {
            operations.numbers.compare(Ordering::is_gt)
        }),
        (Builtin::NumberGe, |operations, _| {
            operations.numbers.compare(Ordering::is_ge)
        }),
        (Builtin::EscapeString, |operations, _| {
            operations.text.escape_string()
        }),
        (Builtin::EscapeChar, |operations, _| {
            operations.text.escape_char()
        }),
        (Builtin::EscapeFormat, |operations, _| {
            operations.text.escape_format()
        }),
        (Builtin::StringStripSuffix, |operations, _| {
            operations.text.string_strip_suffix()
        }),
        (Builtin::StringStripPrefix, |operations, _| {
            operations.text.string_strip_prefix()
        }),
        (Builtin::StringLines, |operations, _| {
            operations.strings.lines()
        }),
        (Builtin::StringTrimStart, |operations, _| {
            operations.strings.trim_start()
        }),
        (Builtin::StringLength, |operations, _| {
            operations.strings.length()
        }),
        (Builtin::StringDrop, |operations, _| {
            operations.strings.drop()
        }),
    ];

    pub fn new(builtin: Builtin, values: Vec<Value>, span: Span, call_site: Span) -> Self {
        Self {
            builtin,
            arguments: Arguments::new(builtin.name(), values, span),
            call_site,
        }
    }

    pub fn run(self, host: &mut dyn CompileTimeHost) -> Result<Value, Diagnostic> {
        let operations = Operations::new(&self.arguments, self.call_site);
        Self::implementation(self.builtin)(&operations, host)
    }

    fn implementation(builtin: Builtin) -> Implementation {
        Self::IMPLEMENTATIONS
            .iter()
            .find(|(implemented, _)| *implemented == builtin)
            .map(|(_, implementation)| *implementation)
            .expect("every builtin has a row in the implementation table")
    }
}

type Implementation =
    for<'a> fn(&Operations<'a>, &mut dyn CompileTimeHost) -> Result<Value, Diagnostic>;

struct Operations<'a> {
    lists: ListOperations<'a>,
    numbers: NumberOperations<'a>,
    strings: StringOperations<'a>,
    text: TextOperations<'a>,
}

impl<'a> Operations<'a> {
    fn new(arguments: &'a Arguments, call_site: Span) -> Self {
        Self {
            lists: ListOperations::new(arguments, call_site),
            numbers: NumberOperations::new(arguments, call_site),
            strings: StringOperations::new(arguments, call_site),
            text: TextOperations::new(arguments, call_site),
        }
    }
}

struct Forms;

impl Forms {
    fn list(items: Vec<Form>, span: Span) -> Value {
        Value::Form(Form::new(FormKind::List(items), span))
    }

    fn count(count: usize, span: Span) -> Value {
        let number = Number::new(count.to_string(), NumberValue::Integer(count as i128));
        Value::Form(Form::new(FormKind::Number(number), span))
    }
}
