use std::rc::Rc;

use rustc_hash::FxHashMap;

use super::ty::Type;
use crate::core::BindingId;

#[derive(Default)]
pub struct TemplateClauses {
    clauses: FxHashMap<BindingId, Vec<(Type, Rc<str>)>>,
}

impl TemplateClauses {
    pub fn add(&mut self, binding: BindingId, clauses: Vec<(Type, Rc<str>)>) {
        self.clauses.insert(binding, clauses);
    }

    pub fn chosen(&self, binding: BindingId, parameter: &Type) -> Option<&Rc<str>> {
        self.clauses
            .get(&binding)?
            .iter()
            .find(|(target, _)| target == parameter)
            .map(|(_, template)| template)
    }
}
