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
    Struct(BindingId),
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

    fn any_part(&self, test: &dyn Fn(&Type) -> bool) -> bool {
        test(self)
            || match self {
                Type::Function(parameters, result) => {
                    parameters.iter().any(|parameter| parameter.any_part(test))
                        || result.any_part(test)
                }
                Type::Ref(inner) | Type::Mut(inner) => inner.any_part(test),
                Type::Var(_) | Type::Generic(_) | Type::Primitive(_) | Type::Struct(_) => false,
            }
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
