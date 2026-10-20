use super::{BindingId, PrimitiveType};
use crate::source::Span;

#[derive(Clone, Debug)]
pub struct TypeExpr {
    kind: TypeExprKind,
    span: Span,
}

#[derive(Clone, Debug)]
pub enum TypeExprKind {
    Primitive(PrimitiveType),
    Named(BindingId),
    Apply(BindingId, Vec<TypeExpr>),
    Function(Vec<TypeExpr>, Box<TypeExpr>),
    Ref(Box<TypeExpr>),
    Mut(Box<TypeExpr>),
    Hole,
    Where(Vec<TypeBinder>, Box<TypeExpr>),
}

#[derive(Clone, Debug)]
pub struct TypeBinder {
    pub variable: BindingId,
    pub bounds: Vec<BindingId>,
    pub span: Span,
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
}
