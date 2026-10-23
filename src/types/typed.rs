use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::display::TypeDisplay;
use super::enums::EnumTable;
use super::rust_types::RustTypeTable;
use super::structs::StructTable;
use super::templates::TemplateBorrows;
use super::traits::TraitTable;
use super::ty::{Bound, GenericId, GenericParam, Scheme, Type};
use crate::core::{
    BindingArena, BindingId, FieldLabel, Literal, ModuleId, ModulePath, ReferenceKind, RustTemplate,
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

    pub fn is_written_form(&self) -> bool {
        matches!(
            self.kind,
            TypedExprKind::Borrow(..) | TypedExprKind::Deref(_)
        )
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
    Recur(Vec<TypedArgument>),
    Try(Box<TypedExpr>),
    Borrow(ReferenceKind, Box<TypedExpr>),
    Deref(Box<TypedExpr>),
    Control(TypedControl),
    Struct(TypedStructExpr),
    Variant(BindingId, Vec<TypedExpr>),
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
    Match(Box<TypedExpr>, Vec<TypedArm>),
}

#[derive(Clone, Debug)]
pub struct TypedArm {
    pub pattern: TypedPattern,
    pub body: TypedBlock,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TypedPattern {
    pub kind: TypedPatternKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum TypedPatternKind {
    Wildcard,
    Bind(TypedBinder),
    Variant(BindingId, Vec<TypedPattern>),
}

impl TypedPattern {
    pub fn binders(&self) -> Vec<&TypedBinder> {
        match &self.kind {
            TypedPatternKind::Wildcard => Vec::new(),
            TypedPatternKind::Bind(binder) => vec![binder],
            TypedPatternKind::Variant(_, patterns) => {
                patterns.iter().flat_map(TypedPattern::binders).collect()
            }
        }
    }

    pub fn binders_mut(&mut self) -> Vec<&mut TypedBinder> {
        match &mut self.kind {
            TypedPatternKind::Wildcard => Vec::new(),
            TypedPatternKind::Bind(binder) => vec![binder],
            TypedPatternKind::Variant(_, patterns) => patterns
                .iter_mut()
                .flat_map(TypedPattern::binders_mut)
                .collect(),
        }
    }
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
    pub wraps: Option<BindingId>,
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
pub struct PendingModeCheck {
    pub callee: BindingId,
    pub position: usize,
    pub kind: ReferenceKind,
    pub span: Span,
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
    pub enums: Vec<BindingId>,
    pub exports: Vec<BindingId>,
}

pub struct TypedProgram {
    pub modules: Vec<TypedModule>,
    pub entry: ModuleId,
    pub bindings: BindingArena,
    pub structs: StructTable,
    pub enums: EnumTable,
    pub rust_types: RustTypeTable,
    pub traits: TraitTable,
    pub generics: HashMap<GenericId, GenericParam>,
    pub implied_bounds: HashMap<GenericId, Vec<Bound>>,
    pub template_borrows: TemplateBorrows,
    pub pending_mode_checks: Vec<PendingModeCheck>,
    pub borrowed_parameters: HashSet<BindingId>,
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
        let structs = self
            .entry_module()
            .structs
            .iter()
            .filter_map(|binding| self.inferred_derive_line(*binding))
            .chain(
                self.entry_module()
                    .enums
                    .iter()
                    .filter_map(|binding| self.inferred_enum_derive_line(*binding)),
            )
            .collect::<String>();
        let definitions: String = self
            .entry_module()
            .definitions
            .iter()
            .map(|definition| {
                let name = self.bindings.get(definition.binding).name();
                format!("(type {name} {})\n", display.scheme(&definition.scheme))
            })
            .collect();
        format!("{structs}{definitions}")
    }

    fn inferred_enum_derive_line(&self, binding: BindingId) -> Option<String> {
        let definition = self.enums.get(binding)?;
        if definition.written_derive.is_some() {
            return None;
        }
        let name = self.bindings.get(binding).name();
        let derives: Vec<&str> = definition
            .derived_traits
            .iter()
            .map(|binding| self.bindings.get(*binding).name())
            .collect();
        Some(format!("(derive {name} {})\n", derives.join(" ")))
    }

    fn inferred_derive_line(&self, binding: BindingId) -> Option<String> {
        let definition = self.structs.get(binding)?;
        if definition.written_derive.is_some() {
            return None;
        }
        let name = self.bindings.get(binding).name();
        let derives: Vec<&str> = definition
            .derived_traits
            .iter()
            .map(|binding| self.bindings.get(*binding).name())
            .collect();
        Some(format!("(derive {name} {})\n", derives.join(" ")))
    }
}
