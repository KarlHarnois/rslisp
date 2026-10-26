use std::fmt;
use std::rc::Rc;

use super::{BindingId, Expr, FieldLabel, Lambda, ParsedTemplate, TypeBinder, TypeBound, TypeExpr};
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
    pub hidden: bool,
}

pub const RUST_TRAIT_METHOD_MARKER: &str = "method";
pub const RUST_TRAIT_HELP_MARKER: &str = "help";

#[derive(Clone, Debug)]
pub struct RustTraitDeclaration {
    pub binding: BindingId,
    pub path: Rc<str>,
    pub self_variable: BindingId,
    pub methods: Vec<TraitMethod>,
    pub help: Option<Rc<str>>,
    pub implementors: Vec<TypeExpr>,
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
    RustTrait(RustTraitDeclaration),
    Trait(
        BindingId,
        Rc<str>,
        BindingId,
        Vec<TypeBound>,
        Vec<TraitAssociatedType>,
        Vec<TraitMethod>,
    ),
    Impl(
        BindingId,
        TypeExpr,
        Vec<ImplAssociatedType>,
        Vec<ImplMethod>,
    ),
    RustType(
        BindingId,
        Vec<TypeBinder>,
        Rc<ParsedTemplate>,
        Vec<(TypeExpr, Rc<ParsedTemplate>)>,
    ),
}

#[derive(Clone, Debug)]
pub struct TraitAssociatedType {
    pub binding: BindingId,
    pub name: Rc<str>,
}

#[derive(Clone, Debug)]
pub struct ImplAssociatedType {
    pub trait_type: BindingId,
    pub name: Rc<str>,
    pub value: TypeExpr,
}

#[derive(Clone, Debug)]
pub struct TraitMethod {
    pub binding: BindingId,
    pub signature: TypeExpr,
    pub default: Option<BindingId>,
}

#[derive(Clone, Debug)]
pub struct ImplMethod {
    pub binding: BindingId,
    pub trait_method: BindingId,
    pub name: Rc<str>,
    pub lambda: Rc<Lambda>,
}

#[derive(Clone, Debug)]
pub struct EnumRust {
    pub path: Rc<str>,
    pub template: Rc<ParsedTemplate>,
    pub template_clause: bool,
    pub bare: bool,
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
