use std::collections::HashMap;
use std::rc::Rc;

use crate::core::{BindingId, PrimitiveType};

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

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Var(TypeVar),
    Generic(GenericId),
    Primitive(PrimitiveType),
    Named(BindingId, Vec<Type>),
    Function(Vec<Type>, Box<Type>),
    Ref(Box<Type>),
    Mut(Box<Type>),
}

impl Type {
    pub fn unit() -> Type {
        Type::Primitive(PrimitiveType::Unit)
    }

    pub fn bool() -> Type {
        Type::Primitive(PrimitiveType::Bool)
    }

    pub fn str_ref() -> Type {
        Type::Ref(Box::new(Type::Primitive(PrimitiveType::Str)))
    }

    pub fn is_reference(&self) -> bool {
        matches!(self, Type::Ref(_) | Type::Mut(_))
    }

    pub fn contains_var(&self, var: TypeVar) -> bool {
        self.any_part(&|part| *part == Type::Var(var))
    }

    pub fn has_vars(&self) -> bool {
        self.any_part(&|part| matches!(part, Type::Var(_)))
    }

    pub fn mentions(&self, generic: GenericId) -> bool {
        self.any_part(&|part| *part == Type::Generic(generic))
    }

    pub fn holds_reference(&self) -> bool {
        self.any_part(&Type::is_reference)
    }

    pub fn stores_reference(&self) -> bool {
        match self {
            Type::Ref(_) | Type::Mut(_) => true,
            Type::Function(..) => false,
            _ => self.parts().into_iter().any(Type::stores_reference),
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
            Type::Function(parameters, result) => Type::Function(
                Self::substitute_all(parameters, replacements),
                Box::new(result.substitute(replacements)),
            ),
            Type::Ref(inner) => Type::Ref(Box::new(inner.substitute(replacements))),
            Type::Mut(inner) => Type::Mut(Box::new(inner.substitute(replacements))),
            Type::Var(_) | Type::Primitive(_) => self.clone(),
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
            Type::Function(parameters, result) => parameters
                .iter()
                .chain(std::iter::once(result.as_ref()))
                .collect(),
            Type::Ref(inner) | Type::Mut(inner) => vec![inner],
            Type::Var(_) | Type::Generic(_) | Type::Primitive(_) => Vec::new(),
        }
    }

    pub fn shares_head_with(&self, other: &Type) -> bool {
        let same_constructor = match (self, other) {
            (Type::Named(left, _), Type::Named(right, _)) => left == right,
            (Type::Var(_) | Type::Generic(_) | Type::Primitive(_), _) => self == other,
            _ => std::mem::discriminant(self) == std::mem::discriminant(other),
        };
        same_constructor && self.parts().len() == other.parts().len()
    }

    fn any_part(&self, test: &dyn Fn(&Type) -> bool) -> bool {
        test(self) || self.parts().into_iter().any(|part| part.any_part(test))
    }
}

#[derive(Clone, Debug)]
pub struct GenericParam {
    pub id: GenericId,
    pub name: Rc<str>,
    pub bounds: Vec<BindingId>,
}

#[derive(Clone, Debug)]
pub struct Scheme {
    pub params: Vec<GenericParam>,
    pub body: Type,
}

impl Scheme {
    pub fn monomorphic(body: Type) -> Self {
        Self {
            params: Vec::new(),
            body,
        }
    }
}
