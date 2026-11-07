use std::rc::Rc;

use rustc_hash::FxHashMap;

use super::traits::TraitTable;
use super::ty::{Bound, Type};
use crate::core::BindingId;

pub enum ClauseTarget {
    Type(Type),
    Trait(Bound),
}

impl ClauseTarget {
    pub fn covers(&self, later: &ClauseTarget, traits: &TraitTable) -> bool {
        match (self, later) {
            (Self::Type(earlier), Self::Type(later)) => earlier == later,
            (Self::Trait(earlier), Self::Type(later)) => {
                traits.satisfies(earlier, later, &|_, _| false)
            }
            (Self::Trait(earlier), Self::Trait(later)) => {
                traits.brings(later, &|brought| brought == earlier)
            }
            (Self::Type(_), Self::Trait(_)) => false,
        }
    }

    fn matches(&self, parameter: &Type, implements: &dyn Fn(&Bound) -> bool) -> bool {
        match self {
            Self::Type(exact) => exact == parameter,
            Self::Trait(bound) => implements(bound),
        }
    }
}

#[derive(Default)]
pub struct TemplateClauses {
    clauses: FxHashMap<BindingId, Vec<(ClauseTarget, Rc<str>)>>,
}

impl TemplateClauses {
    pub fn add(&mut self, binding: BindingId, clauses: Vec<(ClauseTarget, Rc<str>)>) {
        self.clauses.insert(binding, clauses);
    }

    pub fn chosen(
        &self,
        binding: BindingId,
        parameter: &Type,
        implements: &dyn Fn(&Bound) -> bool,
    ) -> Option<&Rc<str>> {
        self.clauses
            .get(&binding)?
            .iter()
            .find(|(target, _)| target.matches(parameter, implements))
            .map(|(_, template)| template)
    }
}
