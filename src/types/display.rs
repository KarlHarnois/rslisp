use rustc_hash::FxHashMap;

use super::projection::ProjectionTable;
use super::ty::{Bound, GenericId, GenericParam, Lifetime, Scheme, Type};
use crate::core::{BindingArena, BindingId};

pub struct TypeDisplay<'a> {
    bindings: &'a BindingArena,
    generics: &'a FxHashMap<GenericId, GenericParam>,
    projections: Option<&'a ProjectionTable>,
}

impl<'a> TypeDisplay<'a> {
    const UNKNOWN: &'static str = "_";

    pub fn new(
        bindings: &'a BindingArena,
        generics: &'a FxHashMap<GenericId, GenericParam>,
    ) -> Self {
        Self {
            bindings,
            generics,
            projections: None,
        }
    }

    pub fn with_projections(self, projections: &'a ProjectionTable) -> Self {
        Self {
            projections: Some(projections),
            ..self
        }
    }

    pub fn show(&self, shown: &Type) -> String {
        if let Some(normalized) = self
            .projections
            .and_then(|projections| projections.normalize(shown))
        {
            return self.show(&normalized);
        }
        self.show_unprojected(shown)
    }

    fn show_unprojected(&self, shown: &Type) -> String {
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
            Type::TraitObject(binding) => self.bindings.get(*binding).name().to_string(),
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

    pub fn scheme_with_receiver(&self, scheme: &Scheme, trait_binding: BindingId) -> String {
        let Type::Function(kind, parameters, result) = &scheme.body else {
            return self.scheme(scheme);
        };
        let mut params = scheme.params.clone();
        let receiver = params.remove(Self::receiver_position(scheme, trait_binding));
        let receiver = Type::shared(Type::Generic(receiver.id));
        let parameters = std::iter::once(receiver)
            .chain(parameters.iter().cloned())
            .collect();
        self.scheme(&Scheme {
            lifetimes: scheme.lifetimes.clone(),
            params,
            body: Type::Function(*kind, parameters, result.clone()),
        })
    }

    fn receiver_position(scheme: &Scheme, trait_binding: BindingId) -> usize {
        scheme
            .params
            .iter()
            .position(|param| {
                param
                    .bounds
                    .iter()
                    .any(|bound| bound.binding == trait_binding)
            })
            .expect("a trait method's scheme binds its trait's `self`")
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
