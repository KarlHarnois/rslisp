use std::fmt;
use std::rc::Rc;

use super::{BindingId, Expr, FieldLabel, ParsedTemplate, TypeBinder, TypeExpr};
use crate::source::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModuleId(u32);

impl ModuleId {
    pub fn new(index: usize) -> Self {
        Self(u32::try_from(index).expect("fewer than 2^32 modules"))
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ModulePath(Rc<str>);

impl ModulePath {
    pub fn new(path: impl Into<Rc<str>>) -> Self {
        Self(path.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModulePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub kind: ItemKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ItemKind {
    Def(BindingId, Expr),
    Signature(BindingId, TypeExpr),
    Struct(BindingId, Vec<TypeBinder>, Vec<(FieldLabel, TypeExpr)>),
    Enum(
        BindingId,
        Vec<TypeBinder>,
        Vec<VariantDeclaration>,
        Option<EnumRust>,
    ),
    Derive(BindingId, Span, Vec<(BindingId, Span)>),
    RustTrait(BindingId, Rc<str>, Vec<TypeExpr>),
    RustType(BindingId, Vec<TypeBinder>, Rc<ParsedTemplate>),
}

#[derive(Clone, Debug)]
pub struct EnumRust {
    pub path: Rc<str>,
    pub template: Rc<ParsedTemplate>,
}

#[derive(Clone, Debug)]
pub struct VariantDeclaration {
    pub binding: BindingId,
    pub name: Rc<str>,
    pub payload: VariantPayload,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum VariantPayload {
    None,
    Positional(Vec<TypeExpr>),
    Named(Vec<(FieldLabel, TypeExpr)>),
}

#[derive(Clone, Debug)]
pub struct CoreModule {
    pub id: ModuleId,
    pub path: ModulePath,
    pub items: Vec<Item>,
    pub exports: Vec<BindingId>,
}
