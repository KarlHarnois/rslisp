use std::borrow::Cow;
use std::slice;

use crate::reader::{Form, FormKind};
use crate::scope::{ChainPrefix, Scope, ScopeChain};

pub(super) struct RestOfBody<'forms> {
    forms: slice::Iter<'forms, Form>,
    let_scopes: ScopeChain,
}

impl<'forms> RestOfBody<'forms> {
    pub(super) fn new(forms: &'forms [Form]) -> Self {
        Self {
            forms: forms.iter(),
            let_scopes: ScopeChain::default(),
        }
    }

    pub(super) fn add_scope(&mut self, scope: Scope) {
        self.let_scopes.push(scope);
    }
}

impl<'forms> Iterator for RestOfBody<'forms> {
    type Item = Cow<'forms, Form>;

    fn next(&mut self) -> Option<Self::Item> {
        let form = self.forms.next()?;
        match self.let_scopes.prefix() {
            Some(prefix) => Some(Cow::Owned(Self::with_prefix(form, &prefix))),
            None => Some(Cow::Borrowed(form)),
        }
    }
}

impl RestOfBody<'_> {
    fn with_prefix(form: &Form, prefix: &ChainPrefix) -> Form {
        let kind = match form.kind() {
            FormKind::Symbol(symbol) => {
                let mut symbol = symbol.clone();
                symbol.scopes_mut().add_prefix(prefix);
                FormKind::Symbol(symbol)
            }
            FormKind::List(items) => FormKind::List(
                items
                    .iter()
                    .map(|item| Self::with_prefix(item, prefix))
                    .collect(),
            ),
            FormKind::String(_) | FormKind::Number(_) => return form.clone(),
        };
        Form::new(kind, form.span())
    }
}
