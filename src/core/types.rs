use super::{BindingId, PrimitiveType, ReferenceKind};
use crate::source::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClosureKind {
    Fn,
    FnMut,
    FnOnce,
}

impl ClosureKind {
    pub fn meets(self, required: Self) -> bool {
        matches!(
            (self, required),
            (Self::Fn, _)
                | (Self::FnMut, Self::FnMut | Self::FnOnce)
                | (Self::FnOnce, Self::FnOnce)
        )
    }

    pub fn keyword(self) -> &'static str {
        match self {
            Self::Fn => "Fn",
            Self::FnMut => "FnMut",
            Self::FnOnce => "FnOnce",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Fn => "fn",
            Self::FnMut => "fn-mut",
            Self::FnOnce => "fn-once",
        }
    }

    pub fn shape(self) -> &'static str {
        match self {
            Self::Fn => "(fn (argument ...) result)",
            Self::FnMut => "(fn-mut (argument ...) result)",
            Self::FnOnce => "(fn-once (argument ...) result)",
        }
    }
}

#[derive(Clone, Debug)]
pub struct TypeExpr {
    kind: TypeExprKind,
    span: Span,
}

#[derive(Clone, Debug)]
pub enum TypeExprKind {
    Primitive(PrimitiveType),
    Named(BindingId, Vec<TypeExpr>),
    Function(ClosureKind, Vec<TypeExpr>, Box<TypeExpr>),
    Reference(ReferenceKind, Option<BindingId>, Box<TypeExpr>),
    Hole,
    Where(Vec<TypeBinder>, Box<TypeExpr>),
    With(Box<TypeExpr>, Box<TypeExpr>),
}

#[derive(Clone, Debug)]
pub struct TypeBinder {
    pub variable: BindingId,
    pub bounds: Vec<TypeBound>,
    pub span: Span,
    pub lifetime: bool,
}

#[derive(Clone, Debug)]
pub struct TypeBound {
    pub binding: BindingId,
    pub argument: Option<TypeExpr>,
}

impl TypeExpr {
    pub fn new(kind: TypeExprKind, span: Span) -> Self {
        Self { kind, span }
    }

    pub fn kind(&self) -> &TypeExprKind {
        &self.kind
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn replacing(&self, variable: BindingId, replacement: &TypeExpr) -> TypeExpr {
        let kind = match self.kind() {
            TypeExprKind::Named(binding, _) if *binding == variable => replacement.kind().clone(),
            TypeExprKind::Named(binding, arguments) => TypeExprKind::Named(
                *binding,
                arguments
                    .iter()
                    .map(|argument| argument.replacing(variable, replacement))
                    .collect(),
            ),
            TypeExprKind::Function(kind, parameters, result) => TypeExprKind::Function(
                *kind,
                parameters
                    .iter()
                    .map(|parameter| parameter.replacing(variable, replacement))
                    .collect(),
                Box::new(result.replacing(variable, replacement)),
            ),
            TypeExprKind::Reference(kind, lifetime, inner) => TypeExprKind::Reference(
                *kind,
                *lifetime,
                Box::new(inner.replacing(variable, replacement)),
            ),
            other => other.clone(),
        };
        TypeExpr::new(kind, self.span)
    }
}
