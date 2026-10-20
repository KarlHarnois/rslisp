use std::collections::HashMap;

use super::ty::{GenericId, GenericParam, Scheme, Type};
use crate::core::BindingArena;

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
            Type::Struct(binding) => self.bindings.get(*binding).name().to_string(),
            Type::Function(parameters, result) => {
                let parameters: Vec<String> = parameters
                    .iter()
                    .map(|parameter| self.show(parameter))
                    .collect();
                format!("(fn ({}) {})", parameters.join(" "), self.show(result))
            }
            Type::Ref(inner) => format!("(ref {})", self.show(inner)),
            Type::Mut(inner) => format!("(mut {})", self.show(inner)),
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
            .map(|bound| self.bindings.get(*bound).name())
            .collect();
        format!("({} {})", param.name, bounds.join(" "))
    }
}
