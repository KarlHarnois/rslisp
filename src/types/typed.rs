use std::collections::HashMap;
use std::rc::Rc;

use super::structs::StructTable;
use super::traits::TraitTable;
use super::ty::{GenericId, GenericParam, Scheme, Type};
use crate::core::{
    BindingArena, BindingId, FieldLabel, Literal, ModuleId, ModulePath, RustTemplate,
};
use crate::source::Span;

#[derive(Clone, Debug)]
pub struct TypedExpr {
    pub kind: TypedExprKind,
    pub ty: Type,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum TypedExprKind {
    Literal(Literal),
    Local(BindingId),
    Global(BindingId),
    Fn(Rc<TypedLambda>),
    Call(Box<TypedExpr>, Vec<TypedArgument>),
    Block(TypedBlock),
    If(Box<TypedExpr>, Box<TypedExpr>, Box<TypedExpr>),
    New(BindingId, Vec<(FieldLabel, TypedExpr)>),
    Field(Box<TypedExpr>, FieldLabel),
    Rust(Rc<RustTemplate>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Borrow {
    None,
    Shared,
    Mutable,
    Passthrough,
    Deref,
}

#[derive(Clone, Debug)]
pub struct TypedArgument {
    pub value: TypedExpr,
    pub borrow: Borrow,
}

#[derive(Clone, Debug)]
pub struct TypedBinder {
    pub binding: BindingId,
    pub ty: Type,
    pub span: Span,
    pub annotated: bool,
}

#[derive(Clone, Debug)]
pub struct TypedLambda {
    pub params: Vec<TypedBinder>,
    pub body: TypedBlock,
}

#[derive(Clone, Debug, Default)]
pub struct TypedBlock {
    pub items: Vec<TypedBodyItem>,
}

#[derive(Clone, Debug)]
pub enum TypedBodyItem {
    Let(TypedBinder, TypedExpr),
    Expr(TypedExpr),
}

#[derive(Clone, Debug)]
pub struct TypedDefinition {
    pub binding: BindingId,
    pub scheme: Scheme,
    pub value: TypedExpr,
    pub span: Span,
}

pub struct TypedModule {
    pub id: ModuleId,
    pub path: ModulePath,
    pub definitions: Vec<TypedDefinition>,
    pub structs: Vec<BindingId>,
    pub exports: Vec<BindingId>,
}

pub struct TypedProgram {
    pub modules: Vec<TypedModule>,
    pub entry: ModuleId,
    pub bindings: BindingArena,
    pub structs: StructTable,
    pub traits: TraitTable,
    pub generics: HashMap<GenericId, GenericParam>,
}

impl TypedProgram {
    pub fn entry_module(&self) -> &TypedModule {
        self.modules
            .iter()
            .find(|module| module.id == self.entry)
            .expect("the entry module is checked")
    }
}
