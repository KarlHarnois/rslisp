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
    Try(Box<Expr>),
    Borrow(ReferenceKind, Box<Expr>),
    Deref(Box<Expr>),
    Control(Control),
    Struct(StructExpr),
    Variant(BindingId, Vec<Expr>),
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
    Match(Box<Expr>, Vec<Arm>),
}

#[derive(Clone, Debug)]
pub struct Arm {
    pub pattern: Pattern,
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum PatternKind {
    Wildcard,
    Bind(BindingId),
    BareName {
        name: Rc<str>,
        binding: BindingId,
        candidates: Vec<BindingId>,
        outer: Option<BindingId>,
    },
    Variant(BindingId, Vec<Pattern>),
    Struct(BindingId, Vec<Pattern>),
    ShortVariant {
        name: Rc<str>,
        candidates: Vec<BindingId>,
        arguments: Vec<Pattern>,
    },
    Literal(Literal),
}

impl Pattern {
    pub fn new(kind: PatternKind, span: Span) -> Self {
        Self { kind, span }
    }
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
            ExprKind::Control(control) => control.children(),
            other => other.nested_children(),
        }
    }
}

impl ExprKind {
    fn nested_children(&self) -> Vec<&Expr> {
        match self {
            ExprKind::Recur(arguments) => arguments.iter().collect(),
            ExprKind::Try(value) | ExprKind::Borrow(_, value) | ExprKind::Deref(value) => {
                vec![value]
            }
            ExprKind::Struct(structure) => structure.children(),
            ExprKind::Variant(_, arguments) => arguments.iter().collect(),
            _ => Vec::new(),
        }
    }
}

impl Control {
    fn children(&self) -> Vec<&Expr> {
        match self {
            Control::Block(block) => block.expressions().collect(),
            Control::If(test, then, otherwise) => vec![test, then, otherwise],
            Control::Match(value, arms) => std::iter::once(value.as_ref())
                .chain(arms.iter().flat_map(|arm| arm.body.expressions()))
                .collect(),
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
