use std::collections::HashMap;
use std::rc::Rc;

use crate::core::{BindingId, ClosureKind, PrimitiveType};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeVar(u32);

impl TypeVar {
    pub fn new(index: usize) -> Self {
        Self(u32::try_from(index).expect("fewer than 2^32 type variables"))
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GenericId(u32);

impl GenericId {
    pub fn new(index: usize) -> Self {
        Self(u32::try_from(index).expect("fewer than 2^32 generic parameters"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lifetime {
    Elided,
    Written(BindingId),
}

impl Lifetime {
    pub fn written(binding: BindingId) -> Self {
        Self::Written(binding)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Var(TypeVar),
    Generic(GenericId),
    Primitive(PrimitiveType),
    Named(BindingId, Vec<Type>),
    Function(ClosureKind, Vec<Type>, Box<Type>),
    Ref(Lifetime, Box<Type>),
    Mut(Lifetime, Box<Type>),
    TraitObject(BindingId),
}

impl Type {
    pub fn unit() -> Type {
        Type::Primitive(PrimitiveType::Unit)
    }

    pub fn bool() -> Type {
        Type::Primitive(PrimitiveType::Bool)
    }

    pub fn str_ref() -> Type {
        Type::shared(Type::Primitive(PrimitiveType::Str))
    }

    pub fn shared(inner: Type) -> Type {
        Type::Ref(Lifetime::Elided, Box::new(inner))
    }

    pub fn mutable(inner: Type) -> Type {
        Type::Mut(Lifetime::Elided, Box::new(inner))
    }

    pub fn reference_lifetime(&self) -> Option<Lifetime> {
        match self {
            Type::Ref(lifetime, _) | Type::Mut(lifetime, _) => Some(*lifetime),
            _ => None,
        }
    }

    pub fn is_unsized(&self) -> bool {
        matches!(self, Type::Primitive(PrimitiveType::Str))
    }

    pub fn is_reference(&self) -> bool {
        matches!(self, Type::Ref(..) | Type::Mut(..))
    }

    pub fn contains_var(&self, var: TypeVar) -> bool {
        self.any_part(&|part| *part == Type::Var(var))
    }

    pub fn is_unknown(&self) -> bool {
        self.has_vars() || matches!(self, Self::Generic(_))
    }

    pub fn without_reference(self) -> Self {
        match self {
            Self::Ref(_, inner) | Self::Mut(_, inner) => *inner,
            other => other,
        }
    }

    pub fn referent(&self) -> &Self {
        match self {
            Self::Ref(_, inner) | Self::Mut(_, inner) => inner,
            other => other,
        }
    }

    pub fn is_concrete(&self) -> bool {
        !self.any_part(&|part| matches!(part, Type::Var(_) | Type::Generic(_)))
    }

    pub fn has_vars(&self) -> bool {
        self.any_part(&|part| matches!(part, Type::Var(_)))
    }

    pub fn mentions(&self, generic: GenericId) -> bool {
        self.any_part(&|part| *part == Type::Generic(generic))
    }

    pub fn contains(&self, part: &Type) -> bool {
        self.any_part(&|candidate| candidate == part)
    }

    pub fn holds_reference(&self) -> bool {
        self.any_part(&Type::is_reference)
    }

    pub fn without_written_lifetimes(&self) -> Type {
        match self {
            Type::Ref(_, inner) => Type::shared(inner.without_written_lifetimes()),
            Type::Mut(_, inner) => Type::mutable(inner.without_written_lifetimes()),
            Type::Named(binding, arguments) => Type::Named(
                *binding,
                arguments
                    .iter()
                    .map(Type::without_written_lifetimes)
                    .collect(),
            ),
            Type::Function(kind, parameters, result) => Type::Function(
                *kind,
                parameters
                    .iter()
                    .map(Type::without_written_lifetimes)
                    .collect(),
                Box::new(result.without_written_lifetimes()),
            ),
            other => other.clone(),
        }
    }

    pub fn substitute_vars(&self, replacements: &HashMap<TypeVar, Type>) -> Type {
        match self {
            Type::Var(var) => replacements
                .get(var)
                .cloned()
                .unwrap_or_else(|| self.clone()),
            Type::Named(binding, arguments) => Type::Named(
                *binding,
                arguments
                    .iter()
                    .map(|argument| argument.substitute_vars(replacements))
                    .collect(),
            ),
            Type::Function(kind, parameters, result) => Type::Function(
                *kind,
                parameters
                    .iter()
                    .map(|parameter| parameter.substitute_vars(replacements))
                    .collect(),
                Box::new(result.substitute_vars(replacements)),
            ),
            Type::Ref(lifetime, inner) => {
                Type::Ref(*lifetime, Box::new(inner.substitute_vars(replacements)))
            }
            Type::Mut(lifetime, inner) => {
                Type::Mut(*lifetime, Box::new(inner.substitute_vars(replacements)))
            }
            Type::Generic(_) | Type::Primitive(_) | Type::TraitObject(_) => self.clone(),
        }
    }

    pub fn substitute(&self, replacements: &HashMap<GenericId, Type>) -> Type {
        match self {
            Type::Generic(generic) => replacements
                .get(generic)
                .cloned()
                .unwrap_or_else(|| self.clone()),
            Type::Named(binding, arguments) => {
                Type::Named(*binding, Self::substitute_all(arguments, replacements))
            }
            Type::Function(kind, parameters, result) => Type::Function(
                *kind,
                Self::substitute_all(parameters, replacements),
                Box::new(result.substitute(replacements)),
            ),
            Type::Ref(lifetime, inner) => {
                Type::Ref(*lifetime, Box::new(inner.substitute(replacements)))
            }
            Type::Mut(lifetime, inner) => {
                Type::Mut(*lifetime, Box::new(inner.substitute(replacements)))
            }
            Type::Var(_) | Type::Primitive(_) | Type::TraitObject(_) => self.clone(),
        }
    }

    fn substitute_all(types: &[Type], replacements: &HashMap<GenericId, Type>) -> Vec<Type> {
        types
            .iter()
            .map(|part| part.substitute(replacements))
            .collect()
    }

    pub fn parts(&self) -> Vec<&Type> {
        match self {
            Type::Named(_, arguments) => arguments.iter().collect(),
            Type::Function(_, parameters, result) => parameters
                .iter()
                .chain(std::iter::once(result.as_ref()))
                .collect(),
            Type::Ref(_, inner) | Type::Mut(_, inner) => vec![inner],
            Type::Var(_) | Type::Generic(_) | Type::Primitive(_) | Type::TraitObject(_) => {
                Vec::new()
            }
        }
    }

    pub fn parameters(&self) -> Option<&[Type]> {
        match self {
            Self::Function(_, parameters, _) => Some(parameters),
            _ => None,
        }
    }

    pub fn shares_head_with(&self, other: &Type) -> bool {
        let same_constructor = match (self, other) {
            (Type::Named(left, _), Type::Named(right, _)) => left == right,
            (Type::Var(_) | Type::Generic(_) | Type::Primitive(_) | Type::TraitObject(_), _) => {
                self == other
            }
            _ => std::mem::discriminant(self) == std::mem::discriminant(other),
        };
        same_constructor && self.parts().len() == other.parts().len()
    }

    fn any_part(&self, test: &dyn Fn(&Type) -> bool) -> bool {
        test(self) || self.parts().into_iter().any(|part| part.any_part(test))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bound {
    pub binding: BindingId,
    pub argument: Option<Type>,
}

impl Bound {
    pub fn plain(binding: BindingId) -> Self {
        Self {
            binding,
            argument: None,
        }
    }

    pub fn substituted(&self, replacements: &HashMap<GenericId, Type>) -> Self {
        Self {
            binding: self.binding,
            argument: self
                .argument
                .as_ref()
                .map(|argument| argument.substitute(replacements)),
        }
    }

    pub fn substituted_vars(&self, replacements: &HashMap<TypeVar, Type>) -> Self {
        Self {
            binding: self.binding,
            argument: self
                .argument
                .as_ref()
                .map(|argument| argument.substitute_vars(replacements)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct GenericParam {
    pub id: GenericId,
    pub name: Rc<str>,
    pub bounds: Vec<Bound>,
}

#[derive(Clone, Debug)]
pub struct Scheme {
    pub lifetimes: Vec<BindingId>,
    pub params: Vec<GenericParam>,
    pub body: Type,
}

impl Scheme {
    pub fn monomorphic(body: Type) -> Self {
        Self {
            lifetimes: Vec::new(),
            params: Vec::new(),
            body,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{BindingArena, BindingInfo, BindingKind};

    struct Substitution {
        replacements: HashMap<TypeVar, Type>,
        variable: TypeVar,
        replacement: Type,
        binding: BindingId,
    }

    impl Substitution {
        fn new() -> Self {
            let variable = TypeVar::new(0);
            let replacement = Type::Primitive(PrimitiveType::I64);
            let mut bindings = BindingArena::default();
            let binding = bindings.add(BindingInfo::new(
                "item".into(),
                BindingKind::TypeVariable,
                None,
            ));
            Self {
                replacements: HashMap::from([(variable, replacement.clone())]),
                variable,
                replacement,
                binding,
            }
        }

        fn variable(&self) -> Type {
            Type::Var(self.variable)
        }

        fn named(&self) -> Type {
            Type::Named(self.binding, vec![self.variable()])
        }

        fn substitute(&self, value: &Type) -> Type {
            value.substitute_vars(&self.replacements)
        }
    }

    #[test]
    fn substitution_reaches_every_part_of_a_type() {
        let fixture = Substitution::new();
        let variable = fixture.variable();
        let replacement = fixture.replacement.clone();
        assert_eq!(
            fixture.substitute(&Type::Function(
                ClosureKind::Fn,
                vec![variable.clone()],
                Box::new(variable.clone())
            )),
            Type::Function(
                ClosureKind::Fn,
                vec![replacement.clone()],
                Box::new(replacement.clone())
            )
        );
        assert_eq!(
            fixture.substitute(&Type::shared(variable.clone())),
            Type::shared(replacement.clone())
        );
        assert_eq!(
            fixture.substitute(&Type::mutable(variable.clone())),
            Type::mutable(replacement.clone())
        );
        assert_eq!(
            fixture.substitute(&fixture.named()),
            Type::Named(fixture.binding, vec![replacement])
        );
    }

    #[test]
    fn only_a_function_has_call_parameters() {
        let parameters = vec![Type::Primitive(PrimitiveType::I64)];
        let function = Type::Function(ClosureKind::Fn, parameters.clone(), Box::new(Type::unit()));
        assert_eq!(function.parameters(), Some(parameters.as_slice()));
        assert_eq!(Type::unit().parameters(), None);
    }

    #[test]
    fn substitution_leaves_other_types_alone() {
        let fixture = Substitution::new();
        let other = TypeVar::new(1);
        assert_eq!(fixture.substitute(&fixture.variable()), fixture.replacement);
        assert_eq!(fixture.substitute(&Type::Var(other)), Type::Var(other));
        assert_eq!(
            fixture.substitute(&Type::Generic(GenericId::new(0))),
            Type::Generic(GenericId::new(0))
        );
        assert_eq!(
            fixture.substitute(&Type::Primitive(PrimitiveType::Bool)),
            Type::Primitive(PrimitiveType::Bool)
        );
    }
}
