use std::rc::Rc;

use super::{BindingId, ReferenceKind, TypeExpr};
use crate::reader::{Form, Number};
use crate::source::Span;

#[derive(Clone, Debug)]
pub struct Expr {
    kind: ExprKind,
    span: Span,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Leaf(Leaf),
    Fn(Rc<Lambda>),
    Call(Box<Expr>, Vec<Expr>),
    Recur(Vec<Expr>),
    Borrow(ReferenceKind, Box<Expr>),
    Deref(Box<Expr>),
    Control(Control),
    Struct(StructExpr),
}

#[derive(Clone, Debug)]
pub enum Leaf {
    Literal(Literal),
    Var(BindingId),
    Quote(Form),
    Rust(Rc<RustTemplate>),
}

#[derive(Clone, Debug)]
pub enum Control {
    Block(Block),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Debug)]
pub enum StructExpr {
    New(BindingId, Vec<(FieldLabel, Expr)>),
    Field(Box<Expr>, FieldLabel),
}

#[derive(Clone, Debug)]
pub enum Literal {
    Number(Number),
    String(Rc<str>),
    Bool(bool),
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

    pub fn children(&self) -> Vec<&Expr> {
        match &self.kind {
            ExprKind::Leaf(_) => Vec::new(),
            ExprKind::Fn(lambda) => lambda.body.expressions().collect(),
            ExprKind::Call(callee, arguments) => {
                std::iter::once(callee.as_ref()).chain(arguments).collect()
            }
            ExprKind::Recur(arguments) => arguments.iter().collect(),
            ExprKind::Borrow(_, value) | ExprKind::Deref(value) => vec![value],
            ExprKind::Control(control) => control.children(),
            ExprKind::Struct(structure) => structure.children(),
        }
    }
}

impl Control {
    fn children(&self) -> Vec<&Expr> {
        match self {
            Control::Block(block) => block.expressions().collect(),
            Control::If(test, then, otherwise) => vec![test, then, otherwise],
        }
    }
}

impl StructExpr {
    fn children(&self) -> Vec<&Expr> {
        match self {
            StructExpr::New(_, fields) => fields.iter().map(|(_, value)| value).collect(),
            StructExpr::Field(value, _) => vec![value],
        }
    }
}

impl Block {
    pub fn expressions(&self) -> impl Iterator<Item = &Expr> {
        self.items.iter().map(BodyItem::expr)
    }
}

impl BodyItem {
    pub fn expr(&self) -> &Expr {
        match self {
            BodyItem::Let(_, value) | BodyItem::Expr(value) => value,
        }
    }
}
