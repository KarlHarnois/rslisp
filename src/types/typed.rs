use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::display::TypeDisplay;
use super::enums::{EnumTable, VariantDefinition, VariantPayload};
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
    Match(TypedMatch),
}

#[derive(Clone, Debug)]
pub struct TypedMatch {
    pub value: Box<TypedExpr>,
    pub arms: Vec<TypedArm>,
    pub unguarded_exhaustive: bool,
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
    pub boxed: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum TypedPatternKind {
    Wildcard,
    Bind(TypedBinder),
    Variant(BindingId, Vec<TypedPattern>),
    Struct(BindingId, Vec<TypedPattern>),
    Literal(Literal, Type),
}

impl TypedPattern {
    pub fn binders(&self) -> Vec<&TypedBinder> {
        match &self.kind {
            TypedPatternKind::Wildcard | TypedPatternKind::Literal(..) => Vec::new(),
            TypedPatternKind::Bind(binder) => vec![binder],
            TypedPatternKind::Variant(_, patterns) | TypedPatternKind::Struct(_, patterns) => {
                patterns.iter().flat_map(TypedPattern::binders).collect()
            }
        }
    }

    pub fn binders_mut(&mut self) -> Vec<&mut TypedBinder> {
        match &mut self.kind {
            TypedPatternKind::Wildcard | TypedPatternKind::Literal(..) => Vec::new(),
            TypedPatternKind::Bind(binder) => vec![binder],
            TypedPatternKind::Variant(_, patterns) | TypedPatternKind::Struct(_, patterns) => {
                patterns
                    .iter_mut()
                    .flat_map(TypedPattern::binders_mut)
                    .collect()
            }
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

impl TypedDefinition {
    pub fn result_error(&self, rust_types: &RustTypeTable) -> Option<&Type> {
        let Type::Function(_, result) = &self.value.ty else {
            return None;
        };
        let Type::Named(binding, arguments) = result.as_ref() else {
            return None;
        };
        if !rust_types.is_result(*binding) || arguments.len() != 2 {
            return None;
        }
        Some(&arguments[1])
    }
}

pub struct TypedModule {
    pub id: ModuleId,
    pub path: ModulePath,
    pub definitions: Vec<TypedDefinition>,
    pub structs: Vec<BindingId>,
    pub enums: Vec<BindingId>,
    pub exports: Vec<BindingId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiverKind {
    Owned,
    Shared,
    Mutable,
}

pub struct TypedTraitMethod {
    pub binding: BindingId,
    pub receiver: Option<ReceiverKind>,
}

pub struct TypedTrait {
    pub binding: BindingId,
    pub module: ModuleId,
    pub rust_name: Rc<str>,
    pub methods: Vec<TypedTraitMethod>,
}

pub struct TraitMethodInfo {
    pub trait_binding: BindingId,
    pub rust_name: Rc<str>,
    pub receiver: Option<ReceiverKind>,
}

#[derive(Clone, Copy)]
pub struct TypedImplMethod {
    pub binding: BindingId,
    pub receiver: Option<ReceiverKind>,
}

pub struct TypedTraitImpl {
    pub trait_binding: BindingId,
    pub implementor: Type,
    pub implementor_span: Span,
    pub module: ModuleId,
    pub methods: Vec<TypedImplMethod>,
}

pub struct TypedProgram {
    pub modules: Vec<TypedModule>,
    pub entry: ModuleId,
    pub bindings: BindingArena,
    pub structs: StructTable,
    pub enums: EnumTable,
    pub rust_types: RustTypeTable,
    pub box_binding: Option<BindingId>,
    pub traits: TraitTable,
    pub trait_definitions: Vec<TypedTrait>,
    pub trait_impls: Vec<TypedTraitImpl>,
    pub trait_methods: HashMap<BindingId, TraitMethodInfo>,
    pub synthetic_bindings: HashSet<BindingId>,
    pub generics: HashMap<GenericId, GenericParam>,
    pub implied_bounds: HashMap<GenericId, Vec<Bound>>,
    pub template_borrows: TemplateBorrows,
    pub pending_mode_checks: Vec<PendingModeCheck>,
    pub borrowed_parameters: HashSet<BindingId>,
    pub warnings: Diagnostics,
}

impl TypedProgram {
    pub fn is_box(&self, ty: &Type) -> bool {
        matches!(ty, Type::Named(binding, arguments) if Some(*binding) == self.box_binding && arguments.len() == 1)
    }

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
            .chain(
                self.entry_module()
                    .enums
                    .iter()
                    .filter_map(|binding| self.inferred_enum_box_line(*binding)),
            )
            .collect::<String>();
        let definitions: String = self
            .entry_module()
            .definitions
            .iter()
            .filter(|definition| !self.synthetic_bindings.contains(&definition.binding))
            .map(|definition| {
                let name = self.bindings.get(definition.binding).name();
                format!("(type {name} {})\n", display.scheme(&definition.scheme))
            })
            .collect();
        format!("{structs}{definitions}")
    }

    fn inferred_enum_box_line(&self, binding: BindingId) -> Option<String> {
        let definition = self.enums.get(binding)?;
        if !definition
            .payload_types()
            .any(|payload| self.is_box(payload))
        {
            return None;
        }
        let display = TypeDisplay::new(&self.bindings, &self.generics);
        let name = self.bindings.get(binding).name();
        let head = if definition.params.is_empty() {
            name.to_string()
        } else {
            let params: Vec<&str> = definition
                .params
                .iter()
                .map(|param| param.name.as_ref())
                .collect();
            format!("({name} {})", params.join(" "))
        };
        let rust = definition
            .rust
            .as_ref()
            .map(|rust| format!(" (rust {:?})", rust.path))
            .unwrap_or_default();
        let variants: Vec<String> = definition
            .variants
            .iter()
            .map(|variant| Self::show_variant(&display, variant))
            .collect();
        Some(format!("(enum {head}{rust} {})\n", variants.join(" ")))
    }

    fn show_variant(display: &TypeDisplay<'_>, variant: &VariantDefinition) -> String {
        let name = &variant.name;
        match &variant.payload {
            VariantPayload::None => name.to_string(),
            VariantPayload::Positional(types) => {
                let types: Vec<String> = types.iter().map(|ty| display.show(ty)).collect();
                format!("({name} {})", types.join(" "))
            }
            VariantPayload::Named(fields) => {
                let fields: Vec<String> = fields
                    .iter()
                    .map(|(label, ty)| format!("({label} {})", display.show(ty)))
                    .collect();
                format!("({name} (struct {}))", fields.join(" "))
            }
        }
    }

    fn inferred_enum_derive_line(&self, binding: BindingId) -> Option<String> {
        let definition = self.enums.get(binding)?;
        if definition.rust.is_some() || definition.written_derive.is_some() {
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
