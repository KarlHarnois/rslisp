use std::collections::HashMap;

use super::ty::{GenericId, GenericParam, Scheme, Type};
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
            Type::Function(parameters, result) => self.function(parameters, result),
            Type::Ref(inner) | Type::Mut(inner) => {
                format!("({} {})", Self::reference_keyword(shown), self.show(inner))
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
        if matches!(reference, Type::Mut(_)) {
            "mut"
        } else {
            "ref"
        }
    }

    pub fn scheme(&self, scheme: &Scheme) -> String {
        if scheme.params.is_empty() {
            return self.show(&scheme.body);
        }
        let params: Vec<String> = scheme
            .params
            .iter()
            .map(|param| self.param(param))
            .collect();
        format!("(where ({}) {})", params.join(" "), self.show(&scheme.body))
    }

    fn param(&self, param: &GenericParam) -> String {
        if param.bounds.is_empty() {
            return param.name.to_string();
        }
        let bounds: Vec<&str> = param
            .bounds
            .iter()
            .map(|bound| self.bindings.get(bound.binding).name())
            .collect();
        format!("({} {})", param.name, bounds.join(" "))
    }
}
