use std::rc::Rc;

use super::problems::ProjectProblems;
use crate::core::Keyword;
use crate::diagnostic::{Diagnostics, SimilarName};
use crate::reader::{Form, FormKind, NumberValue};
use crate::source::Span;

pub struct Field<'f> {
    pub key: &'f str,
    pub keyword: &'f Form,
    pub value: &'f Form,
}

pub struct ScannedForms<'f> {
    pub fields: Fields<'f>,
    pub others: Vec<&'f Form>,
}

pub struct Fields<'f> {
    fields: Vec<Field<'f>>,
}

pub struct FieldScanner<'p> {
    known_keys: &'static [&'static str],
    problems: &'p mut Diagnostics,
}

impl<'p> FieldScanner<'p> {
    pub fn new(known_keys: &'static [&'static str], problems: &'p mut Diagnostics) -> Self {
        Self {
            known_keys,
            problems,
        }
    }

    pub fn scan<'f>(mut self, forms: &'f [Form]) -> ScannedForms<'f> {
        let mut scanned = ScannedForms {
            fields: Fields { fields: Vec::new() },
            others: Vec::new(),
        };
        let mut remaining = forms.iter().peekable();
        while let Some(form) = remaining.next() {
            match Self::key_of(form) {
                Some(key) => {
                    let value = remaining.next_if(|value| Self::key_of(value).is_none());
                    self.add_field(&mut scanned.fields, key, form, value);
                }
                None => scanned.others.push(form),
            }
        }
        scanned
    }

    pub fn key_of(form: &Form) -> Option<&str> {
        Keyword::field_name(form.as_symbol()?.name())
    }

    fn add_field<'f>(
        &mut self,
        fields: &mut Fields<'f>,
        key: &'f str,
        keyword: &'f Form,
        value: Option<&'f Form>,
    ) {
        let Some(value) = value else {
            return self
                .problems
                .push(ProjectProblems::missing_value(key, keyword.span()));
        };
        if !self.known_keys.contains(&key) {
            return self.report_unknown(key, keyword.span());
        }
        if let Some(first) = fields.get(key) {
            return self.problems.push(ProjectProblems::duplicate_entry(
                &format!(":{key}"),
                keyword.span(),
                first.keyword.span(),
            ));
        }
        fields.fields.push(Field {
            key,
            keyword,
            value,
        });
    }

    fn report_unknown(&mut self, key: &str, span: Span) {
        let candidates: Vec<Rc<str>> = self.known_keys.iter().map(|&key| Rc::from(key)).collect();
        let closest = SimilarName::closest(key, &candidates);
        self.problems
            .push(ProjectProblems::unknown_key(key, span, closest.as_deref()));
    }
}

impl<'f> Fields<'f> {
    const STRING: &'static str = "a string";
    const STRINGS: &'static str = "a list of strings";
    const WHOLE_NUMBER: &'static str = "a whole number";

    pub fn get(&self, key: &str) -> Option<&Field<'f>> {
        self.fields.iter().find(|field| field.key == key)
    }

    pub fn string(&self, key: &str, problems: &mut Diagnostics) -> Option<String> {
        let field = self.get(key)?;
        match field.value.kind() {
            FormKind::String(text) => Some(text.to_string()),
            _ => field.report_mismatch(Self::STRING, problems),
        }
    }

    pub fn whole_number(&self, key: &str, problems: &mut Diagnostics) -> Option<(i128, Span)> {
        let field = self.get(key)?;
        match Self::integer_of(field.value) {
            Some(value) => Some((value, field.value.span())),
            None => field.report_mismatch(Self::WHOLE_NUMBER, problems),
        }
    }

    fn integer_of(form: &Form) -> Option<i128> {
        match form.kind() {
            FormKind::Number(number) => match number.value() {
                NumberValue::Integer(value) => Some(*value),
                NumberValue::Float(_) => None,
            },
            _ => None,
        }
    }

    pub fn strings(&self, key: &str, problems: &mut Diagnostics) -> Vec<String> {
        let Some(field) = self.get(key) else {
            return Vec::new();
        };
        let Some(items) = field.value.as_list() else {
            return field
                .report_mismatch(Self::STRINGS, problems)
                .unwrap_or_default();
        };
        items
            .iter()
            .filter_map(|item| Self::quoted(item, problems))
            .collect()
    }

    fn quoted(item: &Form, problems: &mut Diagnostics) -> Option<String> {
        match item.kind() {
            FormKind::String(text) => Some(text.to_string()),
            _ => {
                problems.push(ProjectProblems::unquoted_feature(
                    &item.to_string(),
                    item.span(),
                ));
                None
            }
        }
    }
}

impl Field<'_> {
    fn report_mismatch<T>(&self, expected: &str, problems: &mut Diagnostics) -> Option<T> {
        problems.push(ProjectProblems::mismatched_value(
            self.key,
            expected,
            self.value.span(),
        ));
        None
    }
}
