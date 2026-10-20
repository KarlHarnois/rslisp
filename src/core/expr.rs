use std::rc::Rc;

use super::{BindingId, TypeExpr};
use crate::reader::{Form, Number};
use crate::source::Span;

#[derive(Clone, Debug)]
pub struct Expr {
    kind: ExprKind,
    span: Span,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Literal(Literal),
    Var(BindingId),
    Fn(Rc<Lambda>),
    Call(Box<Expr>, Vec<Expr>),
    Block(Block),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    New(BindingId, Vec<(FieldLabel, Expr)>),
    Field(Box<Expr>, FieldLabel),
    Quote(Form),
    Rust(Rc<RustTemplate>),
}

#[derive(Clone, Debug)]
pub enum Literal {
    Number(Number),
    String(Rc<str>),
}

#[derive(Clone, Debug)]
pub struct FieldLabel {
    pub name: Rc<str>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Binder {
    pub binding: BindingId,
    pub annotation: Option<TypeExpr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Lambda {
    pub params: Vec<Binder>,
    pub body: Block,
}

#[derive(Clone, Debug, Default)]
pub struct Block {
    pub items: Vec<BodyItem>,
}

#[derive(Clone, Debug)]
pub enum BodyItem {
    Let(Binder, Expr),
    Expr(Expr),
}

#[derive(Clone, Debug)]
pub struct RustTemplate {
    pub template: Rc<str>,
    pub type_expr: TypeExpr,
    pub twin: Option<Expr>,
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Self { kind, span }
    }

    pub fn kind(&self) -> &ExprKind {
        &self.kind
    }

    pub fn span(&self) -> Span {
        self.span
    }
}
