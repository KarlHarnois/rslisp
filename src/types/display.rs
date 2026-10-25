use std::collections::HashMap;

use super::ty::{Bound, GenericId, GenericParam, Lifetime, Scheme, Type};
use crate::core::{BindingArena, BindingId};

pub struct TypeDisplay<'a> {
    bindings: &'a BindingArena,
    generics: &'a HashMap<GenericId, GenericParam>,
}

impl<'a> TypeDisplay<'a> {
    const UNKNOWN: &'static str = "_";

    pub fn new(bindings: &'a BindingArena, generics: &'a HashMap<GenericId, GenericParam>) -> Self {
        Self { bindings, generics }
    }

    pub fn show(&self, shown: &Type) -> String {
        match shown {
            Type::Var(_) => Self::UNKNOWN.to_string(),
            Type::Generic(generic) => self
                .generics
                .get(generic)
                .map_or_else(|| Self::UNKNOWN.to_string(), |param| param.name.to_string()),
            Type::Primitive(primitive) => primitive.name().to_string(),
            Type::Named(binding, arguments) => self.named(*binding, arguments),
            Type::Function(_, parameters, result) => self.function(parameters, result),
            Type::Ref(lifetime, inner) | Type::Mut(lifetime, inner) => {
                self.reference(shown, *lifetime, inner)
            }
        }
    }

    fn named(&self, binding: BindingId, arguments: &[Type]) -> String {
        let name = self.bindings.get(binding).name().to_string();
        if arguments.is_empty() {
            return name;
        }
        let arguments: Vec<String> = arguments
            .iter()
            .map(|argument| self.show(argument))
            .collect();
        format!("({name} {})", arguments.join(" "))
    }

    fn function(&self, parameters: &[Type], result: &Type) -> String {
        let parameters: Vec<String> = parameters
            .iter()
            .map(|parameter| self.show(parameter))
            .collect();
        format!("(fn ({}) {})", parameters.join(" "), self.show(result))
    }

    fn reference_keyword(reference: &Type) -> &'static str {
        if matches!(reference, Type::Mut(_, _)) {
            "mut"
        } else {
            "ref"
        }
    }

    fn reference(&self, reference: &Type, lifetime: Lifetime, inner: &Type) -> String {
        let mut parts = vec![Self::reference_keyword(reference).to_string()];
        if let Lifetime::Written(binding) = lifetime {
            parts.push(self.bindings.get(binding).name().to_string());
        }
        parts.push(self.show(inner));
        format!("({})", parts.join(" "))
    }

    pub fn scheme(&self, scheme: &Scheme) -> String {
        let mut declared: Vec<String> = scheme
            .lifetimes
            .iter()
            .map(|binding| self.bindings.get(*binding).name().to_string())
            .collect();
        declared.extend(scheme.params.iter().map(|param| self.param(param)));
        if declared.is_empty() {
            return self.show(&scheme.body);
        }
        format!(
            "(where ({}) {})",
            declared.join(" "),
            self.show(&scheme.body)
        )
    }

    fn param(&self, param: &GenericParam) -> String {
        if param.bounds.is_empty() {
            return param.name.to_string();
        }
        let bounds: Vec<String> = param.bounds.iter().map(|bound| self.bound(bound)).collect();
        format!("({} {})", param.name, bounds.join(" "))
    }

    fn bound(&self, bound: &Bound) -> String {
        let name = self.bindings.get(bound.binding).name();
        match &bound.argument {
            Some(argument) => format!("({name} {})", self.show(argument)),
            None => name.to_string(),
        }
    }
}
