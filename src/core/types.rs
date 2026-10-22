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
}
