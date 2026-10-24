use super::{BindingId, PrimitiveType, ReferenceKind};
use crate::source::Span;

#[derive(Clone, Debug)]
pub struct TypeExpr {
    kind: TypeExprKind,
    span: Span,
}

#[derive(Clone, Debug)]
pub enum TypeExprKind {
    Primitive(PrimitiveType),
    Named(BindingId, Vec<TypeExpr>),
    Function(Vec<TypeExpr>, Box<TypeExpr>),
    Reference(ReferenceKind, Box<TypeExpr>),
    Hole,
    Where(Vec<TypeBinder>, Box<TypeExpr>),
    With(Box<TypeExpr>, Box<TypeExpr>),
}

#[derive(Clone, Debug)]
pub struct TypeBinder {
    pub variable: BindingId,
    pub bounds: Vec<TypeBound>,
    pub span: Span,
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

    pub fn replacing(&self, variable: BindingId, replacement: BindingId) -> TypeExpr {
        let kind = match self.kind() {
            TypeExprKind::Named(binding, arguments) => {
                let arguments = arguments
                    .iter()
                    .map(|argument| argument.replacing(variable, replacement))
                    .collect();
                let binding = if *binding == variable {
                    replacement
                } else {
                    *binding
                };
                TypeExprKind::Named(binding, arguments)
            }
            TypeExprKind::Function(parameters, result) => TypeExprKind::Function(
                parameters
                    .iter()
                    .map(|parameter| parameter.replacing(variable, replacement))
                    .collect(),
                Box::new(result.replacing(variable, replacement)),
            ),
            TypeExprKind::Reference(kind, inner) => {
                TypeExprKind::Reference(*kind, Box::new(inner.replacing(variable, replacement)))
            }
            other => other.clone(),
        };
        TypeExpr::new(kind, self.span)
    }
}
