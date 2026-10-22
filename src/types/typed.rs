use std::collections::HashMap;
use std::rc::Rc;

use super::display::TypeDisplay;
use super::rust_types::RustTypeTable;
use super::structs::StructTable;
use super::traits::TraitTable;
use super::ty::{GenericId, GenericParam, Scheme, Type};
use crate::core::{
    BindingArena, BindingId, FieldLabel, Literal, ModuleId, ModulePath, RustTemplate,
};
use crate::diagnostic::Diagnostics;
use crate::source::Span;

#[derive(Clone, Debug)]
pub struct TypedExpr {
    pub kind: TypedExprKind,
    pub ty: Type,
    pub span: Span,
}

impl TypedExpr {
    pub fn is_place(&self) -> bool {
        self.place_root().is_some()
    }

    pub fn place_root(&self) -> Option<BindingId> {
        match &self.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => Some(*binding),
            TypedExprKind::Struct(TypedStructExpr::Field(owner, _)) => owner.place_root(),
            _ => None,
        }
    }

    pub fn mentions_local(&self, binding: BindingId) -> bool {
        matches!(self.kind, TypedExprKind::Leaf(TypedLeaf::Local(local)) if local == binding)
            || self
                .children()
                .into_iter()
                .any(|child| child.mentions_local(binding))
    }
}

#[derive(Clone, Debug)]
pub enum TypedExprKind {
    Leaf(TypedLeaf),
    Fn(Rc<TypedLambda>),
    Call(Box<TypedExpr>, Vec<TypedArgument>),
    Control(TypedControl),
    Struct(TypedStructExpr),
}

#[derive(Clone, Debug)]
pub enum TypedLeaf {
    Literal(Literal),
    Local(BindingId),
    Global(BindingId),
    Rust(Rc<RustTemplate>),
}

#[derive(Clone, Debug)]
pub enum TypedControl {
    Block(TypedBlock),
    If(Box<TypedExpr>, Box<TypedExpr>, Box<TypedExpr>),
}

#[derive(Clone, Debug)]
pub enum TypedStructExpr {
    New(BindingId, Vec<(FieldLabel, TypedExpr)>),
    Field(Box<TypedExpr>, FieldLabel),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Borrow {
    None,
    Shared,
    Mutable,
    Passthrough,
    Reborrow,
    Deref(usize),
    Collapse(usize),
    Pending(usize),
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
    pub rust_types: RustTypeTable,
    pub traits: TraitTable,
    pub generics: HashMap<GenericId, GenericParam>,
    pub implied_bounds: HashMap<GenericId, Vec<BindingId>>,
    pub warnings: Diagnostics,
}

impl TypedProgram {
    pub fn entry_module(&self) -> &TypedModule {
        self.modules
            .iter()
            .find(|module| module.id == self.entry)
            .expect("the entry module is checked")
    }

    pub fn entry_signatures(&self) -> String {
        let display = TypeDisplay::new(&self.bindings, &self.generics);
        self.entry_module()
            .definitions
            .iter()
            .map(|definition| {
                let name = self.bindings.get(definition.binding).name();
                format!("(type {name} {})\n", display.scheme(&definition.scheme))
            })
            .collect()
    }
}
