use std::collections::HashMap;

use super::traits::Implementor;
use super::ty::{Bound, GenericId, Type};
pub struct Requirement {
    pub bound: Bound,
    pub implementor: Type,
}

pub enum Selection {
    Implemented {
        requirements: Vec<Requirement>,
        argument: Option<Type>,
    },
    Undecided,
    Missing,
}

impl Selection {
    pub fn best(self, other: Selection) -> Selection {
        match (self, other) {
            (implemented @ Selection::Implemented { .. }, _)
            | (_, implemented @ Selection::Implemented { .. }) => implemented,
            (Selection::Undecided, _) | (_, Selection::Undecided) => Selection::Undecided,
            (Selection::Missing, Selection::Missing) => Selection::Missing,
        }
    }
}

pub struct ImplementorMatch<'s> {
    implementor: &'s Implementor,
    bindings: HashMap<GenericId, Type>,
    undecided: bool,
}

impl<'s> ImplementorMatch<'s> {
    pub fn select(
        implementor: &'s Implementor,
        bound: &Bound,
        target: &Type,
        associated: bool,
    ) -> Selection {
        let mut matcher = Self {
            implementor,
            bindings: HashMap::new(),
            undecided: false,
        };
        if !matcher.matches(&implementor.scheme.body, target) {
            return Selection::Missing;
        }
        if associated {
            matcher.associated(bound)
        } else {
            matcher.parameter(bound, target)
        }
    }

    fn associated(&mut self, bound: &Bound) -> Selection {
        if self.undecided {
            return Selection::Undecided;
        }
        let argument = self
            .implementor
            .argument
            .as_ref()
            .unwrap_or(&self.implementor.scheme.body)
            .substitute(&self.bindings);
        if let Some(expected) = &bound.argument
            && !matches!(expected, Type::Var(_))
            && argument != *expected
        {
            return Selection::Missing;
        }
        Selection::Implemented {
            requirements: self.requirements(),
            argument: Some(argument),
        }
    }

    fn parameter(&mut self, bound: &Bound, target: &Type) -> Selection {
        let pattern_argument = self
            .implementor
            .argument
            .as_ref()
            .unwrap_or(&self.implementor.scheme.body);
        let target_argument = bound.argument.as_ref().unwrap_or(target);
        if !self.matches(pattern_argument, target_argument) {
            return Selection::Missing;
        }
        if self.undecided {
            return Selection::Undecided;
        }
        Selection::Implemented {
            requirements: self.requirements(),
            argument: None,
        }
    }

    fn matches(&mut self, pattern: &Type, target: &Type) -> bool {
        match (pattern, target) {
            (Type::Generic(generic), _) if self.is_parameter(*generic) => {
                self.bind(*generic, target)
            }
            (_, Type::Var(_)) => self.leave_undecided(),
            _ => {
                pattern.shares_head_with(target)
                    && self.matches_all(pattern.parts(), target.parts())
            }
        }
    }

    fn matches_all(&mut self, patterns: Vec<&Type>, targets: Vec<&Type>) -> bool {
        patterns
            .into_iter()
            .zip(targets)
            .all(|(pattern, target)| self.matches(pattern, target))
    }

    fn leave_undecided(&mut self) -> bool {
        self.undecided = true;
        true
    }

    fn is_parameter(&self, generic: GenericId) -> bool {
        self.implementor
            .scheme
            .params
            .iter()
            .any(|param| param.id == generic)
    }

    fn bind(&mut self, generic: GenericId, target: &Type) -> bool {
        let bound = self
            .bindings
            .entry(generic)
            .or_insert_with(|| target.clone());
        bound == target
    }

    fn requirements(&self) -> Vec<Requirement> {
        self.implementor
            .scheme
            .params
            .iter()
            .filter_map(|param| Some((param, self.bindings.get(&param.id)?)))
            .flat_map(|(param, implementor)| {
                param.bounds.iter().map(|bound| Requirement {
                    bound: bound.substituted(&self.bindings),
                    implementor: implementor.clone(),
                })
            })
            .collect()
    }
}
