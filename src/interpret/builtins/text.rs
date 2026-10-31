use std::rc::Rc;

use super::Forms;
use super::failure::FailCall;
use crate::diagnostic::{Diagnostic, SimilarName};
use crate::interpret::arguments::Arguments;
use crate::interpret::{CompileTimeHost, Value};
use crate::reader::{Form, FormKind, Symbol};
use crate::source::Span;

pub struct TextOperations<'a> {
    arguments: &'a Arguments,
    call_site: Span,
}

impl<'a> TextOperations<'a> {
    const ESCAPES: [(char, &'static str); 6] = [
        ('"', "\\\""),
        ('\\', "\\\\"),
        ('\n', "\\n"),
        ('\r', "\\r"),
        ('\t', "\\t"),
        ('\0', "\\0"),
    ];
    const STRING_BRACES: [(char, &'static str); 2] = [('{', "{{"), ('}', "}}")];
    const FORMAT_BRACES: [(char, &'static str); 2] = [('{', "{{{{"), ('}', "}}}}")];

    pub fn new(arguments: &'a Arguments, call_site: Span) -> Self {
        Self {
            arguments,
            call_site,
        }
    }

    pub fn is_kind(&self, matches: fn(&FormKind) -> bool) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        let is_match = self
            .arguments
            .value(0)
            .as_form()
            .is_some_and(|form| matches(form.kind()));
        Ok(Value::Bool(is_match))
    }

    pub fn is_list(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        Ok(Value::Bool(self.arguments.value(0).is_list()))
    }

    pub fn symbol_name(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        let name = self.arguments.symbol(0)?.name().to_string();
        Ok(self.string_value(name))
    }

    pub fn is_keyword(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        Ok(Value::Bool(
            self.arguments.value(0).keyword_name().is_some(),
        ))
    }

    pub fn keyword_name(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        let name = self.arguments.keyword_name(0)?.to_string();
        Ok(self.string_value(name))
    }

    pub fn keyword_field(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        Ok(Value::Form(self.arguments.keyword_field(0)?))
    }

    pub fn field_keyword(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        Ok(Value::Form(self.arguments.field_keyword(0)?))
    }

    pub fn make_symbol(&self) -> Result<Value, Diagnostic> {
        let context = self.arguments.symbol(0)?;
        let name = self.joined_parts(1)?;
        let symbol = Symbol::with_scopes(name, context.scopes().clone());
        Ok(Value::Form(Form::new(
            FormKind::Symbol(symbol),
            self.call_site,
        )))
    }

    pub fn make_string(&self) -> Result<Value, Diagnostic> {
        Ok(self.string_value(self.joined_parts(0)?))
    }

    pub fn same_binding(&self, host: &mut dyn CompileTimeHost) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let left = self.arguments.symbol(0)?;
        let right = self.arguments.symbol(1)?;
        Ok(Value::Bool(host.same_binding(left, right)))
    }

    pub fn definition_symbol(&self, host: &mut dyn CompileTimeHost) -> Result<Value, Diagnostic> {
        let term = self.arguments.form(0)?;
        let context = self.arguments.symbol(0)?;
        let symbol = host.definition_symbol(context, self.joined_parts(1)?, term.span())?;
        Ok(self.symbol_value(symbol))
    }

    pub fn type_definition_symbol(
        &self,
        host: &mut dyn CompileTimeHost,
    ) -> Result<Value, Diagnostic> {
        let term = self.arguments.symbol(0)?;
        let symbol = host.type_definition_symbol(term, self.joined_parts(1)?);
        Ok(self.symbol_value(symbol))
    }

    fn symbol_value(&self, symbol: Symbol) -> Value {
        Value::Form(Form::new(FormKind::Symbol(symbol), self.call_site))
    }

    pub fn is_bound(&self, host: &mut dyn CompileTimeHost) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        Ok(Value::Bool(host.is_bound(self.arguments.symbol(0)?)))
    }

    pub fn closest_name(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let name = self.arguments.symbol(0)?.name();
        let candidates = self.arguments.symbol_forms(1)?;
        let names: Vec<Rc<str>> = candidates
            .iter()
            .filter_map(Form::as_symbol)
            .map(|symbol| Rc::from(symbol.name()))
            .collect();
        let closest = SimilarName::closest(name, &names);
        let found = candidates
            .iter()
            .filter(|candidate| candidate.as_symbol().map(Symbol::name) == closest.as_deref())
            .take(1)
            .cloned()
            .collect();
        Ok(Forms::list(found, self.call_site))
    }

    pub fn fail(&self) -> Result<Value, Diagnostic> {
        Err(FailCall::new(self.arguments).failure())
    }

    pub fn value_eq(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let left = self.arguments.value(0);
        let right = self.arguments.value(1);
        let equal = match (left.to_form(), right.to_form()) {
            (Some(left), Some(right)) => Self::forms_equal(&left, &right),
            _ => Self::plain_values_equal(left, right),
        };
        Ok(Value::Bool(equal))
    }

    fn plain_values_equal(left: &Value, right: &Value) -> bool {
        match (left, right) {
            (Value::Bool(left), Value::Bool(right)) => left == right,
            (Value::Unit, Value::Unit) => true,
            _ => false,
        }
    }

    pub fn not(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        Ok(Value::Bool(!self.arguments.boolean(0)?))
    }

    pub fn escape_string(&self) -> Result<Value, Diagnostic> {
        self.escape(&Self::STRING_BRACES)
    }

    pub fn escape_char(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        let text = self.arguments.string(0)?;
        let mut escaped = String::new();
        for character in text.chars().flat_map(char::escape_debug) {
            match character {
                '{' => escaped.push_str("{{"),
                '}' => escaped.push_str("}}"),
                _ => escaped.push(character),
            }
        }
        Ok(self.string_value(escaped))
    }

    pub fn escape_format(&self) -> Result<Value, Diagnostic> {
        self.escape(&Self::FORMAT_BRACES)
    }

    pub fn string_strip_suffix(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let text = self.arguments.string(0)?;
        let suffix = self.arguments.string(1)?;
        Ok(self.string_value(text.strip_suffix(suffix).unwrap_or(text).to_string()))
    }

    pub fn string_strip_prefix(&self) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(2)?;
        let text = self.arguments.string(0)?;
        let prefix = self.arguments.string(1)?;
        Ok(self.string_value(text.strip_prefix(prefix).unwrap_or(text).to_string()))
    }

    fn escape(&self, braces: &[(char, &'static str); 2]) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        let text = self.arguments.string(0)?;
        let mut escaped = String::new();
        for character in text.chars() {
            match Self::ESCAPES
                .iter()
                .chain(braces)
                .find(|(from, _)| *from == character)
            {
                Some((_, escape)) => escaped.push_str(escape),
                None => escaped.push(character),
            }
        }
        Ok(self.string_value(escaped))
    }

    fn forms_equal(left: &Form, right: &Form) -> bool {
        match (left.kind(), right.kind()) {
            (FormKind::Symbol(left), FormKind::Symbol(right)) => left.name() == right.name(),
            (FormKind::String(left), FormKind::String(right)) => left == right,
            (FormKind::Number(left), FormKind::Number(right)) => left.value() == right.value(),
            (FormKind::List(left), FormKind::List(right)) => {
                left.len() == right.len()
                    && left
                        .iter()
                        .zip(right)
                        .all(|(left, right)| Self::forms_equal(left, right))
            }
            _ => false,
        }
    }

    fn joined_parts(&self, start: usize) -> Result<String, Diagnostic> {
        (start..self.arguments.values().len())
            .map(|index| self.part_text(index))
            .collect()
    }

    fn part_text(&self, index: usize) -> Result<String, Diagnostic> {
        let form = self.arguments.form(index)?;
        Ok(match form.kind() {
            FormKind::Symbol(symbol) => symbol.name().to_string(),
            FormKind::String(text) => text.to_string(),
            FormKind::Number(number) => number.text().to_string(),
            FormKind::List(_) => form.to_string(),
        })
    }

    fn string_value(&self, text: String) -> Value {
        Value::Form(Form::new(FormKind::String(text.into()), self.call_site))
    }
}
