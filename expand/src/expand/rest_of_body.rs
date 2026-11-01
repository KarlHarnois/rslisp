use std::borrow::Cow;
use std::slice;

use crate::reader::Form;
use crate::scope::{Scope, ScopeChain};

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
            Some(prefix) => {
                let mut form = form.clone();
                form.add_prefix(&prefix);
                Some(Cow::Owned(form))
            }
            None => Some(Cow::Borrowed(form)),
        }
    }
}
