use std::rc::Rc;

use rustc_hash::FxHashMap;

use super::selection::{ImplementorMatch, PatternMatch};
use super::traits::Implementor;
use super::ty::Type;
use crate::core::BindingId;

pub struct AssociatedType {
    pub trait_binding: BindingId,
    pub parameter: Rc<str>,
    pub rust_name: Rc<str>,
}

struct ProjectionRule {
    implementor: Implementor,
    value: Type,
}

impl ProjectionRule {
    fn value_for(&self, owner: &Type) -> Option<Type> {
        match ImplementorMatch::match_pattern(
            &self.implementor,
            &self.implementor.scheme.body,
            owner,
        ) {
            PatternMatch::Matched(bindings) => Some(self.value.substitute(&bindings)),
            PatternMatch::Undecided | PatternMatch::Missing => None,
        }
    }

    fn admits(&self, candidate: &Type) -> bool {
        !matches!(
            ImplementorMatch::match_pattern(&self.implementor, &self.value, candidate),
            PatternMatch::Missing
        )
    }
}

#[derive(Default)]
pub struct ProjectionTable {
    associated: FxHashMap<BindingId, AssociatedType>,
    rules: FxHashMap<BindingId, Vec<ProjectionRule>>,
}

impl ProjectionTable {
    pub fn declare(&mut self, projection: BindingId, associated: AssociatedType) {
        self.associated.insert(projection, associated);
    }

    pub fn add_rule(&mut self, projection: BindingId, implementor: &Implementor, value: Type) {
        let rule = ProjectionRule {
            implementor: Implementor {
                scheme: implementor.scheme.clone(),
                argument: implementor.argument.clone(),
            },
            value,
        };
        self.rules.entry(projection).or_default().push(rule);
    }

    pub fn associated(&self, projection: BindingId) -> Option<&AssociatedType> {
        self.associated.get(&projection)
    }

    pub fn is_projection(&self, binding: BindingId) -> bool {
        self.associated.contains_key(&binding)
    }

    pub fn declared_by(&self, trait_binding: BindingId) -> Vec<BindingId> {
        let mut declared: Vec<BindingId> = self
            .associated
            .iter()
            .filter(|(_, associated)| associated.trait_binding == trait_binding)
            .map(|(projection, _)| *projection)
            .collect();
        declared.sort();
        declared
    }

    pub fn normalize(&self, projected: &Type) -> Option<Type> {
        let (projection, owner) = Self::projection_parts(projected)?;
        self.normalize_projection(projection, owner)
    }

    pub fn normalize_projection(&self, projection: BindingId, owner: &Type) -> Option<Type> {
        self.rules
            .get(&projection)?
            .iter()
            .find_map(|rule| rule.value_for(owner))
    }

    pub fn normalize_deep(&self, shown: &Type) -> Type {
        let normalized = match shown {
            Type::Named(binding, arguments) => Type::Named(*binding, self.normalize_all(arguments)),
            Type::Function(kind, parameters, result) => Type::Function(
                *kind,
                self.normalize_all(parameters),
                Box::new(self.normalize_deep(result)),
            ),
            Type::Ref(lifetime, inner) => {
                Type::Ref(*lifetime, Box::new(self.normalize_deep(inner)))
            }
            Type::Mut(lifetime, inner) => {
                Type::Mut(*lifetime, Box::new(self.normalize_deep(inner)))
            }
            other => other.clone(),
        };
        self.normalize(&normalized)
            .map_or(normalized, |value| self.normalize_deep(&value))
    }

    fn normalize_all(&self, shown: &[Type]) -> Vec<Type> {
        shown.iter().map(|part| self.normalize_deep(part)).collect()
    }

    pub fn awaits_owner(&self, projected: &Type, candidate: &Type) -> bool {
        let Some((projection, owner)) = Self::projection_parts(projected) else {
            return false;
        };
        owner.has_vars()
            && self
                .rules
                .get(&projection)
                .is_some_and(|rules| rules.iter().any(|rule| rule.admits(candidate)))
    }

    fn projection_parts(projected: &Type) -> Option<(BindingId, &Type)> {
        match projected {
            Type::Named(projection, arguments) => match arguments.as_slice() {
                [owner] => Some((*projection, owner)),
                _ => None,
            },
            _ => None,
        }
    }
}
