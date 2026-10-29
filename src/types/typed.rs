use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::display::TypeDisplay;
use super::enums::{EnumTable, VariantDefinition, VariantPayload};
use super::rust_types::RustTypeTable;
use super::structs::StructTable;
use super::templates::TemplateBorrows;
use super::traits::TraitTable;
use super::ty::{Bound, GenericId, GenericParam, Lifetime, Scheme, Type};
use super::unifier::LiteralKind;
use crate::core::{
    BindingArena, BindingId, ClosureKind, FieldLabel, Literal, ModuleId, ModulePath, ReferenceKind,
    RustTemplate,
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
        self.field_place().map(|place| place.root)
    }

    pub fn field_place(&self) -> Option<FieldPlace<'_>> {
        match &self.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => Some(FieldPlace {
                root: *binding,
                root_type: &self.ty,
                path: Vec::new(),
            }),
            TypedExprKind::Struct(TypedStructExpr::Field(owner, label)) => {
                let mut place = owner.field_place()?;
                place.path.push(label);
                Some(place)
            }
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

pub struct FieldPlace<'a> {
    pub root: BindingId,
    pub root_type: &'a Type,
    pub path: Vec<&'a FieldLabel>,
}

impl FieldPlace<'_> {
    pub fn reads_field(&self) -> bool {
        !self.path.is_empty()
    }
}

#[derive(Clone, Debug)]
pub enum TypedExprKind {
    Leaf(TypedLeaf),
    Fn(Rc<TypedLambda>),
    Call(Box<TypedExpr>, Vec<TypedArgument>),
    Recur(Vec<TypedArgument>),
    Try {
        value: Box<TypedExpr>,
        conversion: TryErrorConversion,
    },
    Borrow(ReferenceKind, Box<TypedExpr>),
    Deref(Box<TypedExpr>),
    Control(TypedControl),
    Struct(TypedStructExpr),
    Variant(BindingId, Vec<TypedExpr>),
    Assoc(Box<TypedExpr>, Vec<TypedExpr>, Box<TypedExpr>),
    TraitObject(Box<TypedExpr>, BindingId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TryErrorConversion {
    Identity,
    Converted,
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
    Update(BindingId, Box<TypedExpr>, Vec<(FieldLabel, TypedExpr)>),
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
    pub kind: ClosureKind,
    pub moved_capture: Option<(BindingId, Span)>,
    pub changed_capture: Option<(BindingId, Span)>,
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
    pub written_result: bool,
}

impl TypedDefinition {
    pub fn result_error(&self, enums: &EnumTable) -> Option<&Type> {
        let Type::Function(_, _, result) = &self.value.ty else {
            return None;
        };
        let Type::Named(binding, arguments) = result.as_ref() else {
            return None;
        };
        let definition = enums.get(*binding)?;
        (definition.is_result_shaped() && arguments.len() == 2).then(|| &arguments[1])
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

#[derive(Clone)]
pub struct TypedTraitMethod {
    pub binding: BindingId,
    pub receiver: Option<ReceiverKind>,
    pub default: Option<BindingId>,
    pub returns_self: bool,
}

#[derive(Clone)]
pub struct TypedAssociatedType {
    pub binding: BindingId,
    pub name: Rc<str>,
}

pub struct TypedTrait {
    pub binding: BindingId,
    pub module: ModuleId,
    pub rust_name: Rc<str>,
    pub supertraits: Vec<Bound>,
    pub associated: Vec<TypedAssociatedType>,
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
    pub params: Vec<GenericParam>,
    pub associated: Vec<(BindingId, Rc<str>, Type)>,
    pub implementor_span: Span,
    pub module: ModuleId,
    pub methods: Vec<TypedImplMethod>,
}

#[derive(Clone)]
pub struct TypedConversion {
    pub enum_binding: BindingId,
    pub variant: usize,
    pub error: Type,
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
    pub rust_trait_paths: HashMap<BindingId, Rc<str>>,
    pub trait_impls: Vec<TypedTraitImpl>,
    pub conversions: Vec<TypedConversion>,
    pub trait_methods: HashMap<BindingId, TraitMethodInfo>,
    pub synthetic_bindings: HashSet<BindingId>,
    pub generics: HashMap<GenericId, GenericParam>,
    pub implied_bounds: HashMap<GenericId, Vec<Bound>>,
    pub associated_generics: HashMap<GenericId, Rc<str>>,
    pub template_borrows: TemplateBorrows,
    pub pending_mode_checks: Vec<PendingModeCheck>,
    pub borrowed_parameters: HashSet<BindingId>,
    pub stored_function_parameters: HashSet<BindingId>,
    pub escaping_lambdas: HashSet<BindingId>,
    pub borrowing_lambdas: HashMap<BindingId, HashSet<BindingId>>,
    pub mutable_function_bindings: HashSet<BindingId>,
    pub literal_generics: HashMap<GenericId, LiteralKind>,
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

    pub fn has_lifetime_parameter(&self, binding: BindingId) -> bool {
        if let Some(definition) = self.structs.get(binding) {
            return !definition.lifetimes.is_empty() || self.definition_holds_borrow(binding);
        }
        if let Some(definition) = self.enums.get(binding) {
            return definition.rust.is_none()
                && (!definition.lifetimes.is_empty() || self.definition_holds_borrow(binding));
        }
        false
    }

    pub fn function_type_needs_shared_lifetime(&self, parameters: &[Type], result: &Type) -> bool {
        self.function_type_returns_borrow(result) && self.parameter_lifetimes(parameters) > 1
    }

    pub fn function_type_needs_outer_lifetime(&self, parameters: &[Type], result: &Type) -> bool {
        self.function_type_returns_borrow(result) && self.parameter_lifetimes(parameters) == 0
    }

    fn type_needs_outer_lifetime(&self, ty: &Type) -> bool {
        matches!(ty, Type::Function(_, parameters, result)
            if self.function_type_needs_outer_lifetime(parameters, result))
    }

    fn parameter_lifetimes(&self, parameters: &[Type]) -> usize {
        parameters
            .iter()
            .map(|parameter| self.borrow_lifetimes(parameter))
            .sum()
    }

    pub fn function_type_returns_borrow(&self, result: &Type) -> bool {
        self.borrow_lifetimes(result) > 0
    }

    pub fn is_function_returning_borrow(&self, ty: &Type) -> bool {
        matches!(ty, Type::Function(_, _, result) if self.function_type_returns_borrow(result))
    }

    pub fn borrow_lifetimes(&self, ty: &Type) -> usize {
        match ty {
            Type::Ref(lifetime, inner) | Type::Mut(lifetime, inner) => {
                usize::from(*lifetime == Lifetime::Elided) + self.borrow_lifetimes(inner)
            }
            Type::Named(binding, arguments) => {
                usize::from(self.has_lifetime_parameter(*binding))
                    + arguments
                        .iter()
                        .map(|argument| self.borrow_lifetimes(argument))
                        .sum::<usize>()
            }
            Type::TraitObject(_) => 0,
            Type::Function(..) | Type::Var(_) | Type::Generic(_) | Type::Primitive(_) => 0,
        }
    }

    pub fn type_holds_borrow(&self, ty: &Type) -> bool {
        self.type_holds_borrow_with(ty, &mut HashSet::new())
    }

    fn type_holds_borrow_with(&self, ty: &Type, visiting: &mut HashSet<BindingId>) -> bool {
        match ty {
            Type::Ref(..) | Type::Mut(..) => true,
            Type::Function(..) => false,
            Type::Named(binding, arguments) => {
                arguments
                    .iter()
                    .any(|argument| self.type_holds_borrow_with(argument, visiting))
                    || self.definition_holds_borrow_with(*binding, visiting)
            }
            Type::Var(_) | Type::Generic(_) | Type::Primitive(_) | Type::TraitObject(_) => false,
        }
    }

    pub fn definition_holds_borrow(&self, binding: BindingId) -> bool {
        self.definition_holds_borrow_with(binding, &mut HashSet::new())
    }

    fn definition_holds_borrow_with(
        &self,
        binding: BindingId,
        visiting: &mut HashSet<BindingId>,
    ) -> bool {
        if !visiting.insert(binding) {
            return false;
        }
        let holds = if let Some(definition) = self.structs.get(binding) {
            definition
                .fields
                .iter()
                .any(|(_, field)| self.stored_type_holds_borrow(field, visiting))
        } else if let Some(definition) = self.enums.get(binding) {
            definition.rust.is_none()
                && definition
                    .payload_types()
                    .any(|payload| self.stored_type_holds_borrow(payload, visiting))
        } else {
            false
        };
        visiting.remove(&binding);
        holds
    }

    fn stored_type_holds_borrow(&self, stored: &Type, visiting: &mut HashSet<BindingId>) -> bool {
        self.type_holds_borrow_with(stored, visiting) || self.type_needs_outer_lifetime(stored)
    }

    pub fn entry_signatures(&self) -> String {
        let display = TypeDisplay::new(&self.bindings, &self.generics)
            .with_projections(self.traits.projections());
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
            .filter_map(|definition| {
                let name = self.writable_name(definition.binding)?;
                Some(format!(
                    "(type {name} {})\n",
                    display.scheme(&definition.scheme)
                ))
            })
            .collect();
        format!("{structs}{definitions}")
    }

    fn writable_name(&self, binding: BindingId) -> Option<&str> {
        if !self.bindings.is_introduced(binding) {
            return Some(self.bindings.get(binding).name());
        }
        let answering = self.bindings.answering_macro(binding)?;
        Some(self.bindings.get(answering).name())
    }

    fn inferred_enum_box_line(&self, binding: BindingId) -> Option<String> {
        let definition = self.enums.get(binding)?;
        if !definition
            .payload_types()
            .any(|payload| self.is_box(payload))
        {
            return None;
        }
        let display = TypeDisplay::new(&self.bindings, &self.generics)
            .with_projections(self.traits.projections());
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
            .map(|rust| {
                let bare = if rust.bare { " bare" } else { "" };
                format!(" (rust {:?}{bare})", rust.path)
            })
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
