use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::copy::InferredCopies;
use super::display::TypeDisplay;
use super::enums::{EnumDefinition, EnumTable, VariantDefinition, VariantPayload};
use super::graph::DependencyGraph;
use super::pending::PendingBorrow;
use super::problems::{MismatchContext, TypeProblems};
use super::rust_types::{RustType, RustTypeTable};
use super::selection::Selection;
use super::structs::{StructDefinition, StructTable};
use super::templates::TemplateBorrows;
use super::traits::{Implementor, RustTrait, TraitTable};
use super::ty::{Bound, GenericId, GenericParam, Scheme, Type, TypeVar};
use super::typed::{
    PendingModeCheck, ReceiverKind, TraitMethodInfo, TypedConversion, TypedDefinition, TypedExpr,
    TypedExprKind, TypedLeaf, TypedTrait, TypedTraitImpl, TypedTraitMethod,
};
use super::unifier::{LiteralKind, Obligation, Unifier};
use crate::core::{
    BindingArena, BindingId, BindingKind, BodyItem, CopyTrait, CoreModule, Expr, ExprKind, Item,
    ItemKind, Leaf, ModuleId, ParsedTemplate, PrimitiveType, ReferenceKind, StructDerives,
    StructExpr, TypeBinder, TypeExpr, TypeExprKind, VariantDeclaration,
    VariantPayload as CoreVariantPayload,
};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::source::Span;

pub(super) enum Admissible {
    Unbounded,
    Open(Vec<Type>),
    Closed(Vec<Type>),
}

#[derive(Clone, Copy)]
pub(super) enum ObligationPass {
    Deferring,
    Final,
}

struct ParameterUsage {
    outside_self: bool,
    first_recursive: Option<Span>,
}

impl ParameterUsage {
    fn scan(variants: &[VariantDeclaration], enum_binding: BindingId, variable: BindingId) -> Self {
        let mut usage = Self {
            outside_self: false,
            first_recursive: None,
        };
        for variant in variants {
            usage.visit_payload(&variant.payload, enum_binding, variable, false);
        }
        usage
    }

    fn visit_payload(
        &mut self,
        payload: &CoreVariantPayload,
        enum_binding: BindingId,
        variable: BindingId,
        inside_self: bool,
    ) {
        match payload {
            CoreVariantPayload::None => {}
            CoreVariantPayload::Positional(types) => {
                for type_expr in types {
                    self.visit(type_expr, enum_binding, variable, inside_self);
                }
            }
            CoreVariantPayload::Named(fields) => {
                for (_, type_expr) in fields {
                    self.visit(type_expr, enum_binding, variable, inside_self);
                }
            }
        }
    }

    fn visit(
        &mut self,
        type_expr: &TypeExpr,
        enum_binding: BindingId,
        variable: BindingId,
        inside_self: bool,
    ) {
        match type_expr.kind() {
            TypeExprKind::Named(binding, arguments) => self.visit_named(
                type_expr,
                *binding,
                arguments,
                enum_binding,
                variable,
                inside_self,
            ),
            TypeExprKind::Function(_, parameters, result) => {
                for parameter in parameters {
                    self.visit(parameter, enum_binding, variable, inside_self);
                }
                self.visit(result, enum_binding, variable, inside_self);
            }
            TypeExprKind::Reference(_, _, inner) => {
                self.visit(inner, enum_binding, variable, inside_self);
            }
            TypeExprKind::Primitive(_) | TypeExprKind::Hole => {}
            TypeExprKind::Where(..) | TypeExprKind::With(..) | TypeExprKind::Trait(_) => {}
        }
    }

    fn visit_named(
        &mut self,
        type_expr: &TypeExpr,
        binding: BindingId,
        arguments: &[TypeExpr],
        enum_binding: BindingId,
        variable: BindingId,
        inside_self: bool,
    ) {
        let inside_self = inside_self || binding == enum_binding;
        if binding == variable {
            self.record_use(type_expr.span(), inside_self);
        }
        for argument in arguments {
            self.visit(argument, enum_binding, variable, inside_self);
        }
    }

    fn record_use(&mut self, span: Span, inside_self: bool) {
        if inside_self {
            self.first_recursive.get_or_insert(span);
        } else {
            self.outside_self = true;
        }
    }
}

pub struct TypeChecker {
    pub(super) bindings: BindingArena,
    pub(super) unifier: Unifier,
    pub(super) structs: StructTable,
    pub(super) enums: EnumTable,
    pub(super) rust_types: RustTypeTable,
    pub(super) traits: TraitTable,
    pub(super) generics: HashMap<GenericId, GenericParam>,
    pub(super) schemes: HashMap<BindingId, Scheme>,
    pub(super) component: HashMap<BindingId, Type>,
    pub(super) locals: HashMap<BindingId, Type>,
    pub(super) variant_arm: HashMap<BindingId, VariantArmReference>,
    pub(super) compile_time_only: HashSet<BindingId>,
    failed_bindings: HashSet<BindingId>,
    pub(super) diagnostics: Diagnostics,
    pub(super) warnings: Diagnostics,
    missing_annotations: Diagnostics,
    pub(super) type_arities: HashMap<BindingId, usize>,
    trait_arguments: HashSet<BindingId>,
    signatures: HashMap<BindingId, (TypeExpr, Span)>,
    monomorphic: Vec<Type>,
    implied_bounds: HashMap<GenericId, Vec<Bound>>,
    pub(super) associated_generics: HashMap<GenericId, std::rc::Rc<str>>,
    pub(super) field_accessors: HashMap<BindingId, Rc<str>>,
    pub(super) pending_borrows: Vec<PendingBorrow>,
    pub(super) template_borrows: TemplateBorrows,
    pub(super) recur_target: Option<super::infer::RecurTarget>,
    pub(super) try_target: Option<super::infer::TryTarget>,
    pub(super) changed_structs: HashSet<BindingId>,
    pub(super) inferred_copies: InferredCopies,
    macro_origins: HashMap<Span, Rc<str>>,
    copy_trait: Option<BindingId>,
    box_binding: Option<BindingId>,
    pub(super) written_parameters: HashMap<BindingId, Vec<bool>>,
    pub(super) pending_mode_checks: Vec<PendingModeCheck>,
    pub(super) trait_definitions: Vec<TypedTrait>,
    pub(super) rust_trait_paths: HashMap<BindingId, Rc<str>>,
    pub(super) trait_impls: Vec<TypedTraitImpl>,
    impl_methods: HashSet<BindingId>,
    impl_generics: HashSet<GenericId>,
    pub(super) conversions: Vec<TypedConversion>,
    pub(super) trait_methods: HashMap<BindingId, TraitMethodInfo>,
    pub(super) synthetic_bindings: HashSet<BindingId>,
    pub(super) pinned: HashMap<BindingId, Type>,
}

struct Definition<'p> {
    binding: BindingId,
    value: &'p Expr,
    span: Span,
}

struct UserTrait<'a> {
    binding: BindingId,
    rust_name: &'a std::rc::Rc<str>,
    self_variable: BindingId,
    supertraits: &'a [crate::core::TypeBound],
    associated: &'a [crate::core::TraitAssociatedType],
    methods: &'a [crate::core::TraitMethod],
}

type LoweredFields = Vec<(Rc<str>, Type)>;
type LoweredImplementor = (Type, Vec<GenericParam>, Vec<(BindingId, Type)>);

#[derive(Clone, Copy)]
pub(super) struct VariantArmReference {
    pub outer: Option<BindingId>,
    pub variant: BindingId,
}

impl TypeChecker {
    const PARAMETER_NAMES: &str = "abcdefghijklmnopqrstuvwxyz";
    pub const MAIN: &str = "main";

    pub(super) fn new(bindings: BindingArena) -> Self {
        Self {
            bindings,
            unifier: Unifier::default(),
            structs: StructTable::default(),
            enums: EnumTable::default(),
            rust_types: RustTypeTable::default(),
            traits: TraitTable::default(),
            generics: HashMap::new(),
            schemes: HashMap::new(),
            component: HashMap::new(),
            locals: HashMap::new(),
            variant_arm: HashMap::new(),
            compile_time_only: HashSet::new(),
            failed_bindings: HashSet::new(),
            diagnostics: Diagnostics::default(),
            warnings: Diagnostics::default(),
            missing_annotations: Diagnostics::default(),
            type_arities: HashMap::new(),
            trait_arguments: HashSet::new(),
            signatures: HashMap::new(),
            monomorphic: Vec::new(),
            implied_bounds: HashMap::new(),
            associated_generics: HashMap::new(),
            field_accessors: HashMap::new(),
            pending_borrows: Vec::new(),
            template_borrows: TemplateBorrows::default(),
            recur_target: None,
            try_target: None,
            changed_structs: HashSet::new(),
            inferred_copies: InferredCopies::default(),
            macro_origins: HashMap::new(),
            copy_trait: None,
            box_binding: None,
            written_parameters: HashMap::new(),
            pending_mode_checks: Vec::new(),
            trait_definitions: Vec::new(),
            rust_trait_paths: HashMap::new(),
            trait_impls: Vec::new(),
            impl_methods: HashSet::new(),
            impl_generics: HashSet::new(),
            conversions: Vec::new(),
            trait_methods: HashMap::new(),
            synthetic_bindings: HashSet::new(),
            pinned: HashMap::new(),
        }
    }

    pub(super) fn collect_declarations(&mut self, modules: &[CoreModule]) {
        self.record_type_arities(modules);
        self.record_trait_arguments(modules);
        for module in modules {
            for item in &module.items {
                if let Err(diagnostic) = self.collect_item(module.id, item) {
                    self.diagnostics.push(diagnostic);
                }
            }
        }
        self.box_binding = self.rust_types.box_binding();
        if let Some(box_binding) = self.box_binding {
            super::boxes::BoxPlanner::add_boxes(&mut self.structs, &mut self.enums, box_binding);
        }
        self.collect_trait_impls(modules);
        self.register_derived_traits(modules);
        self.report_supertraits();
        self.check_trait_claims(modules);
        self.copy_trait = self.traits.with_paths(&[CopyTrait::PATH]).first().copied();
        self.record_written_parameters(modules);
    }

    fn record_written_parameters(&mut self, modules: &[CoreModule]) {
        for item in modules.iter().flat_map(|module| &module.items) {
            let ItemKind::Def(binding, value) = &item.kind else {
                continue;
            };
            let ExprKind::Fn(lambda) = value.kind() else {
                continue;
            };
            let signature = self.signatures.get(binding).map(|(signature, _)| signature);
            let written = lambda
                .params
                .iter()
                .enumerate()
                .map(|(position, param)| {
                    param.annotation.is_some()
                        || signature.is_some_and(|signature| {
                            Self::signature_parameter_is_written(signature, position)
                        })
                })
                .collect();
            self.written_parameters.insert(*binding, written);
        }
    }

    fn signature_parameter_is_written(signature: &TypeExpr, position: usize) -> bool {
        match signature.kind() {
            TypeExprKind::Function(_, parameters, _) => parameters
                .get(position)
                .is_none_or(|parameter| !matches!(parameter.kind(), TypeExprKind::Hole)),
            _ => true,
        }
    }

    fn collect_item(&mut self, module: ModuleId, item: &Item) -> Result<(), Diagnostic> {
        match &item.kind {
            ItemKind::Def(..) | ItemKind::Signature(..) | ItemKind::Derive(..) => {
                self.collect_value_item(item)
            }
            _ => self.collect_type_item(module, item),
        }
    }

    fn collect_value_item(&mut self, item: &Item) -> Result<(), Diagnostic> {
        if let ItemKind::Signature(binding, signature) = &item.kind {
            self.signatures
                .insert(*binding, (signature.clone(), item.span));
            return Ok(());
        }
        if let ItemKind::Derive(..) = &item.kind {
            return Ok(());
        }
        if let ItemKind::Def(binding, value) = &item.kind {
            self.record_field_accessor(*binding, value);
        }
        Ok(())
    }

    fn collect_type_item(&mut self, module: ModuleId, item: &Item) -> Result<(), Diagnostic> {
        let kind = &item.kind;
        if let ItemKind::Struct(binding, params, fields) = kind {
            return self.collect_struct(*binding, params, fields);
        }
        if let ItemKind::Enum(binding, params, variants, rust) = kind {
            return self.collect_enum(*binding, params, variants, rust);
        }
        if let ItemKind::RustTrait(declaration) = kind {
            return self.collect_rust_trait(declaration);
        }
        if let ItemKind::RustType(binding, binders, template, clauses) = kind {
            return self.collect_rust_type(*binding, binders, template, clauses);
        }
        if let ItemKind::Trait(
            binding,
            rust_name,
            self_variable,
            supertraits,
            associated,
            methods,
        ) = kind
        {
            return self.collect_user_trait(
                module,
                UserTrait {
                    binding: *binding,
                    rust_name,
                    self_variable: *self_variable,
                    supertraits,
                    associated,
                    methods,
                },
            );
        }
        Ok(())
    }

    fn record_trait_arguments(&mut self, modules: &[CoreModule]) {
        for item in modules.iter().flat_map(|module| &module.items) {
            let ItemKind::RustTrait(declaration) = &item.kind else {
                continue;
            };
            if declaration.implementors.iter().any(Self::entry_uses_with) {
                self.trait_arguments.insert(declaration.binding);
            }
        }
    }

    fn entry_uses_with(entry: &TypeExpr) -> bool {
        match entry.kind() {
            TypeExprKind::With(..) => true,
            TypeExprKind::Where(_, inner) => Self::entry_uses_with(inner),
            _ => false,
        }
    }

    pub(super) fn trait_takes_argument(&self, binding: BindingId) -> bool {
        self.trait_arguments.contains(&binding)
    }

    fn check_trait_claims(&mut self, modules: &[CoreModule]) {
        for item in modules.iter().flat_map(|module| &module.items) {
            let ItemKind::RustTrait(declaration) = &item.kind else {
                continue;
            };
            let trait_binding = &declaration.binding;
            let path = &declaration.path;
            for implementor in &declaration.implementors {
                let implementor = Self::implemented_type(implementor);
                let TypeExprKind::Named(binding, _) = implementor.kind() else {
                    continue;
                };
                let Some(definition) = self.structs.get(*binding) else {
                    continue;
                };
                if definition
                    .derives
                    .iter()
                    .any(|derived| *derived == StructDerives::derive_name(path))
                {
                    continue;
                }
                let struct_name = self.bindings.get(*binding).name().to_string();
                let trait_name = self.bindings.get(*trait_binding).name().to_string();
                self.diagnostics.push(TypeProblems::trait_claim_on_struct(
                    &struct_name,
                    &trait_name,
                    implementor.span(),
                ));
            }
        }
    }

    fn implemented_type(implementor: &TypeExpr) -> &TypeExpr {
        match implementor.kind() {
            TypeExprKind::With(body, _) => body,
            TypeExprKind::Where(_, inner) => Self::implemented_type(inner),
            _ => implementor,
        }
    }

    fn record_field_accessor(&mut self, binding: BindingId, value: &Expr) {
        let ExprKind::Fn(lambda) = value.kind() else {
            return;
        };
        let [parameter] = lambda.params.as_slice() else {
            return;
        };
        let [BodyItem::Expr(body)] = lambda.body.items.as_slice() else {
            return;
        };
        let ExprKind::Struct(StructExpr::Field(owner, label)) = body.kind() else {
            return;
        };
        let ExprKind::Leaf(Leaf::Var(owner_binding)) = owner.kind() else {
            return;
        };
        if *owner_binding == parameter.binding {
            self.field_accessors.insert(binding, label.name.clone());
        }
    }

    fn record_type_arities(&mut self, modules: &[CoreModule]) {
        for item in modules.iter().flat_map(|module| &module.items) {
            if let ItemKind::Struct(binding, params, _)
            | ItemKind::Enum(binding, params, _, _)
            | ItemKind::RustType(binding, params, ..) = &item.kind
            {
                let arity = params.iter().filter(|binder| !binder.lifetime).count();
                self.type_arities.insert(*binding, arity);
            }
        }
    }

    pub(super) fn check_definitions(
        &mut self,
        modules: &[CoreModule],
        entry: ModuleId,
    ) -> HashMap<BindingId, TypedDefinition> {
        let definitions: Vec<Definition> = modules
            .iter()
            .flat_map(|module| &module.items)
            .filter_map(|item| match &item.kind {
                ItemKind::Def(binding, value) => Some(Definition {
                    binding: *binding,
                    value,
                    span: item.span,
                }),
                _ => None,
            })
            .collect();
        let pairs: Vec<(BindingId, &Expr)> = definitions
            .iter()
            .map(|definition| (definition.binding, definition.value))
            .collect();
        let graph = DependencyGraph::build(&pairs, &self.bindings);
        let order: Vec<BindingId> = definitions
            .iter()
            .map(|definition| definition.binding)
            .collect();
        let runtime = graph.reachable_from(&self.main_definition(modules, entry));
        self.compile_time_only = order
            .iter()
            .copied()
            .filter(|binding| graph.is_compile_time_only(*binding) && !runtime.contains(binding))
            .collect();
        let by_binding: HashMap<BindingId, &Definition> = definitions
            .iter()
            .map(|definition| (definition.binding, definition))
            .collect();
        let mut typed = HashMap::new();
        for component in graph.components(&order) {
            let members: Vec<&Definition> = component
                .iter()
                .filter(|binding| !self.compile_time_only.contains(binding))
                .map(|binding| by_binding[binding])
                .collect();
            let bindings: Vec<BindingId> = members.iter().map(|member| member.binding).collect();
            if graph.depends_on_any(&bindings, &self.failed_bindings) {
                self.failed_bindings.extend(bindings);
                continue;
            }
            typed.extend(self.check_component(&members));
        }
        typed
    }

    fn main_definition(&self, modules: &[CoreModule], entry: ModuleId) -> Vec<BindingId> {
        modules
            .iter()
            .filter(|module| module.id == entry)
            .flat_map(|module| &module.items)
            .filter_map(|item| match &item.kind {
                ItemKind::Def(binding, _) if self.bindings.get(*binding).name() == Self::MAIN => {
                    Some(*binding)
                }
                _ => None,
            })
            .collect()
    }

    pub(super) fn admissible_types(&self, generic: GenericId) -> Admissible {
        let traits: Vec<&RustTrait> = self
            .generics
            .get(&generic)
            .map(|param| {
                param
                    .bounds
                    .iter()
                    .filter_map(|bound| self.traits.get(bound.binding))
                    .collect()
            })
            .unwrap_or_default();
        let Some((first, others)) = traits.split_first() else {
            return Admissible::Unbounded;
        };
        let mut admissible: Vec<Type> = first
            .implementors
            .iter()
            .map(|implementor| implementor.scheme.body.clone())
            .collect();
        for rust_trait in others {
            admissible.retain(|candidate| {
                rust_trait
                    .implementors
                    .iter()
                    .any(|implementor| implementor.scheme.body == *candidate)
            });
        }
        if traits.iter().any(|rust_trait| rust_trait.is_open()) {
            Admissible::Open(admissible)
        } else {
            Admissible::Closed(admissible)
        }
    }

    pub(super) fn copy_rules(&self) -> super::copy::CopyRules<'_> {
        super::copy::CopyRules::new(&self.traits, &self.generics, &self.structs, &self.enums)
    }

    pub(super) fn is_box(&self, ty: &Type) -> bool {
        matches!(ty, Type::Named(binding, arguments) if Some(*binding) == self.box_binding && arguments.len() == 1)
    }

    pub(super) fn visible_payload(&self, ty: &Type) -> Type {
        match ty {
            Type::Named(binding, arguments)
                if Some(*binding) == self.box_binding && arguments.len() == 1 =>
            {
                arguments[0].clone()
            }
            _ => ty.clone(),
        }
    }

    pub(super) fn visible_payloads(
        &self,
        owner: BindingId,
        index: usize,
        arguments: &[Type],
    ) -> Vec<Type> {
        self.enums
            .get(owner)
            .and_then(|definition| definition.instantiated_payload(index, arguments))
            .unwrap_or_default()
            .into_iter()
            .map(|payload| self.visible_payload(&payload))
            .collect()
    }

    pub(super) fn require_copy(&mut self, value: &Type) {
        let Type::Var(var) = self.unifier.shallow(value) else {
            return;
        };
        if let Some(copy) = self.copy_trait {
            self.unifier.add_bound(var, Bound::plain(copy));
        }
    }

    pub(super) fn display(&self) -> TypeDisplay<'_> {
        TypeDisplay::new(&self.bindings, &self.generics)
    }

    pub(super) fn defer_missing_annotation(&mut self, diagnostic: Diagnostic) {
        self.missing_annotations.push(diagnostic);
    }

    fn resolve_missing_annotations(&mut self) {
        if !self.diagnostics.has_errors() {
            self.diagnostics
                .extend(std::mem::take(&mut self.missing_annotations));
        }
    }

    pub(super) fn macro_name(&self, span: Span) -> Option<Rc<str>> {
        self.macro_origins.get(&span).cloned()
    }

    pub(super) fn new_generic(&mut self, name: &str, bounds: Vec<Bound>) -> GenericParam {
        let id = GenericId::new(self.generics.len());
        let param = GenericParam {
            id,
            name: name.into(),
            bounds: bounds.clone(),
        };
        self.generics.insert(id, param.clone());
        self.unifier.register_generic(id, bounds);
        param
    }

    pub(super) fn discharge_obligations(&mut self, pass: ObligationPass) {
        for obligation in self.unifier.take_obligations() {
            self.discharge(obligation, pass);
        }
    }

    fn discharge(&mut self, obligation: Obligation, pass: ObligationPass) {
        let bound = self.resolve_bound(&obligation.bound);
        let implementor = self.unifier.resolve(&obligation.implementor);
        match implementor {
            Type::Var(var) => self.unifier.add_bound(var, bound),
            Type::Generic(generic) => self.check_generic_bound(generic, bound, obligation.span),
            _ => self.select_implementor(bound, &implementor, obligation.span, pass),
        }
    }

    fn resolve_bound(&self, bound: &Bound) -> Bound {
        Bound {
            binding: bound.binding,
            argument: bound
                .argument
                .as_ref()
                .map(|argument| self.unifier.resolve(argument)),
        }
    }

    fn select_implementor(
        &mut self,
        bound: Bound,
        implementor: &Type,
        span: Span,
        pass: ObligationPass,
    ) {
        if self.is_boxed_trait(implementor, bound.binding) {
            return;
        }
        match self.traits.select(&bound, implementor) {
            Selection::Implemented {
                requirements,
                argument,
            } => {
                if let (Some(argument), Some(expected)) = (argument, &bound.argument)
                    && self.unifier.unify(expected, &argument, span).is_err()
                {
                    self.report_missing_trait(&bound, implementor, span);
                    return;
                }
                for requirement in requirements {
                    let required = Obligation {
                        bound: requirement.bound,
                        implementor: requirement.implementor,
                        span,
                    };
                    self.discharge(required, pass);
                }
            }
            Selection::Missing if !implementor.has_vars() => {
                self.report_missing_trait(&bound, implementor, span)
            }
            Selection::Undecided | Selection::Missing => self.leave_unresolved(
                Obligation {
                    bound,
                    implementor: implementor.clone(),
                    span,
                },
                pass,
            ),
        }
    }

    fn leave_unresolved(&mut self, obligation: Obligation, pass: ObligationPass) {
        match pass {
            ObligationPass::Deferring => self.unifier.defer(obligation),
            ObligationPass::Final => {
                self.defer_missing_annotation(TypeProblems::cannot_infer(obligation.span))
            }
        }
    }

    fn check_component(&mut self, members: &[&Definition]) -> Vec<(BindingId, TypedDefinition)> {
        let inferred: Vec<&Definition> = members
            .iter()
            .copied()
            .filter(|member| !self.assign_rust_scheme(member))
            .collect();
        let signatures: HashMap<BindingId, Scheme> = inferred
            .iter()
            .filter_map(|member| self.prepare_member(member))
            .collect();
        let mut typed: Vec<(BindingId, TypedExpr)> = Vec::new();
        for member in &inferred {
            match self.infer_member(member) {
                Ok(value) => typed.push((member.binding, value)),
                Err(diagnostic) => {
                    self.diagnostics.push(diagnostic);
                    self.failed_bindings.insert(member.binding);
                    let failed = self.failed_scheme();
                    self.schemes.insert(member.binding, failed);
                }
            }
        }
        let decided = self.decide_pending_borrows();
        for (binding, value) in &mut typed {
            decided.patch(value);
            self.borrow_unsized_result(*binding, value);
        }
        self.discharge_obligations(ObligationPass::Deferring);
        let generics: Vec<(BindingId, &TypedExpr)> = typed
            .iter()
            .map(|(binding, value)| (*binding, value))
            .collect();
        if !generics.is_empty() {
            let signature_count = signatures
                .values()
                .map(|signature| signature.params.len())
                .max()
                .unwrap_or_default();
            self.generalize_component(&generics, signature_count);
        }
        let mut results = Vec::new();
        for (binding, value) in typed {
            let signature = signatures.get(&binding);
            let written_result = signature.is_some_and(Self::writes_function_result);
            let scheme = self.finish_scheme(binding, &value, signature);
            self.schemes.insert(binding, scheme.clone());
            let span = members
                .iter()
                .find(|member| member.binding == binding)
                .map_or(value.span, |member| member.span);
            results.push((
                binding,
                TypedDefinition {
                    binding,
                    scheme,
                    value,
                    span,
                    written_result,
                },
            ));
        }
        results.extend(
            members
                .iter()
                .filter_map(|member| self.rust_definition(member)),
        );
        self.component.clear();
        results
    }

    fn writes_function_result(signature: &Scheme) -> bool {
        matches!(
            &signature.body,
            crate::types::Type::Function(_, _, result)
                if matches!(
                    result.as_ref(),
                    crate::types::Type::Function(kind, _, _) if *kind != crate::core::ClosureKind::Fn
                )
        )
    }

    fn borrow_unsized_result(&mut self, binding: BindingId, value: &mut TypedExpr) {
        let Type::Function(kind, parameters, result) = self.unifier.resolve(&value.ty) else {
            return;
        };
        if !result.is_unsized() {
            return;
        }
        let borrowed = Type::Function(kind, parameters, Box::new(Type::shared(*result)));
        value.ty = borrowed.clone();
        self.component.insert(binding, borrowed);
    }

    fn assign_rust_scheme(&mut self, member: &Definition) -> bool {
        let ExprKind::Leaf(Leaf::Rust(template)) = member.value.kind() else {
            return false;
        };
        self.template_borrows
            .register(member.binding, &template.template);
        let scheme = match self.lower_scheme(&template.type_expr) {
            Ok(scheme) => scheme,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                self.failed_scheme()
            }
        };
        self.schemes.insert(member.binding, scheme);
        true
    }

    fn rust_definition(&self, member: &Definition) -> Option<(BindingId, TypedDefinition)> {
        let ExprKind::Leaf(Leaf::Rust(template)) = member.value.kind() else {
            return None;
        };
        let scheme = self.schemes.get(&member.binding)?.clone();
        let value = TypedExpr {
            kind: TypedExprKind::Leaf(TypedLeaf::Rust(template.clone())),
            ty: scheme.body.clone(),
            span: member.value.span(),
        };
        Some((
            member.binding,
            TypedDefinition {
                binding: member.binding,
                scheme,
                value,
                span: member.span,
                written_result: false,
            },
        ))
    }

    fn prepare_member(&mut self, member: &Definition) -> Option<(BindingId, Scheme)> {
        if let Some((signature, _)) = self.signatures.get(&member.binding).cloned() {
            return match self.lower_scheme(&signature) {
                Ok(scheme) => {
                    if self.impl_methods.contains(&member.binding) {
                        self.impl_generics
                            .extend(scheme.params.iter().map(|param| param.id));
                    }
                    self.component.insert(member.binding, scheme.body.clone());
                    Some((member.binding, scheme))
                }
                Err(diagnostic) => {
                    self.diagnostics.push(diagnostic);
                    let fresh = self.unifier.fresh();
                    self.component.insert(member.binding, fresh);
                    None
                }
            };
        }
        if let Some(pinned) = self.pinned.get(&member.binding).cloned() {
            self.component.insert(member.binding, pinned);
            return None;
        }
        let shape = self.unwritten_shape(member.value);
        self.component.insert(member.binding, shape);
        None
    }

    fn unwritten_shape(&mut self, value: &Expr) -> Type {
        let ExprKind::Fn(lambda) = value.kind() else {
            return self.unifier.fresh();
        };
        let parameters = lambda.params.iter().map(|_| self.unifier.fresh()).collect();
        Type::Function(
            crate::core::ClosureKind::Fn,
            parameters,
            Box::new(self.unifier.fresh()),
        )
    }

    fn infer_member(&mut self, member: &Definition) -> Result<TypedExpr, Diagnostic> {
        self.locals.clear();
        self.variant_arm.clear();
        let expected = self.component[&member.binding].clone();
        let context = match self.signatures.get(&member.binding) {
            Some((_, signature)) => MismatchContext::Signature {
                signature: *signature,
            },
            None => MismatchContext::Plain,
        };
        self.infer_against(member.value, &expected, context)
    }

    fn finish_scheme(
        &mut self,
        binding: BindingId,
        value: &TypedExpr,
        signature: Option<&Scheme>,
    ) -> Scheme {
        let inferred = self.unifier.resolve(&self.component[&binding]);
        if let Some(signature) = signature {
            return Scheme {
                lifetimes: signature.lifetimes.clone(),
                params: signature.params.clone(),
                body: inferred,
            };
        }
        if matches!(value.kind, TypedExprKind::Fn(_)) {
            let mut params = self.mentioned_generics(&inferred);
            for param in &mut params {
                for bound in &mut param.bounds {
                    bound.argument = bound
                        .argument
                        .as_ref()
                        .map(|argument| self.unifier.resolve(argument));
                }
            }
            return Scheme {
                lifetimes: Vec::new(),
                params,
                body: inferred,
            };
        }
        self.monomorphic.push(inferred.clone());
        Scheme::monomorphic(inferred)
    }

    fn generalize_component(&mut self, members: &[(BindingId, &TypedExpr)], start: usize) {
        let vars = self.component_vars(members);
        let span = members
            .first()
            .map(|(_, value)| value.span)
            .expect("a recursive group has at least one member");
        self.generalize_vars(vars, start, span);
        self.discharge_obligations(ObligationPass::Deferring);
    }

    fn component_vars(&self, members: &[(BindingId, &TypedExpr)]) -> Vec<TypeVar> {
        let excluded = self.monomorphic_vars();
        let mut vars = Vec::new();
        for (binding, value) in members {
            if self.generalizes(binding, value) {
                self.add_free_vars(binding, &excluded, &mut vars);
            }
        }
        vars
    }

    fn generalizes(&self, binding: &BindingId, value: &TypedExpr) -> bool {
        !self.signatures.contains_key(binding) && matches!(value.kind, TypedExprKind::Fn(_))
    }

    fn add_free_vars(
        &self,
        binding: &BindingId,
        excluded: &HashSet<TypeVar>,
        vars: &mut Vec<TypeVar>,
    ) {
        for var in self.free_vars(&self.component[binding]) {
            if !excluded.contains(&var) && !vars.contains(&var) {
                vars.push(var);
            }
        }
    }

    fn generalize_vars(&mut self, vars: Vec<TypeVar>, start: usize, span: Span) {
        self.link_bounded_vars(&vars, span);
        let mut position = start;
        for var in vars {
            if self.generalize_var(var, position, span).is_some() {
                position += 1;
            }
        }
    }

    fn link_bounded_vars(&mut self, vars: &[TypeVar], span: Span) {
        while self.link_round(vars, span) {}
    }

    fn link_round(&mut self, vars: &[TypeVar], span: Span) -> bool {
        let mut linked = false;
        for var in vars {
            if self.is_unbound(*var) {
                linked |= self.link_var(*var, vars, span);
            }
        }
        linked
    }

    fn link_var(&mut self, var: TypeVar, vars: &[TypeVar], span: Span) -> bool {
        let mut linked = false;
        for other in self.linked_vars(var, vars) {
            if self.is_unbound(var)
                && self.is_unbound(other)
                && self
                    .unifier
                    .unify(&Type::Var(var), &Type::Var(other), span)
                    .is_ok()
            {
                linked = true;
            }
        }
        linked
    }

    fn linked_vars(&self, var: TypeVar, vars: &[TypeVar]) -> Vec<TypeVar> {
        self.unifier
            .bounds(var)
            .iter()
            .filter(|bound| {
                self.traits
                    .get(bound.binding)
                    .is_none_or(|rust_trait| rust_trait.associated.is_none())
            })
            .filter_map(|bound| {
                let argument = bound
                    .argument
                    .as_ref()
                    .map(|argument| self.unifier.shallow(argument));
                match argument {
                    Some(Type::Var(other)) if other != var && vars.contains(&other) => Some(other),
                    _ => None,
                }
            })
            .collect()
    }

    fn is_unbound(&self, var: TypeVar) -> bool {
        matches!(self.unifier.shallow(&Type::Var(var)), Type::Var(current) if current == var)
    }

    fn mentioned_generics(&self, inferred: &Type) -> Vec<GenericParam> {
        let mut ids = Vec::new();
        Self::collect_generics(inferred, &mut ids);
        ids.into_iter()
            .filter_map(|id| self.generics.get(&id).cloned())
            .collect()
    }

    fn collect_generics(value: &Type, ids: &mut Vec<GenericId>) {
        match value {
            Type::Generic(id) => {
                if !ids.contains(id) {
                    ids.push(*id);
                }
            }
            _ => value
                .parts()
                .into_iter()
                .for_each(|part| Self::collect_generics(part, ids)),
        }
    }

    fn generalize_var(
        &mut self,
        var: TypeVar,
        position: usize,
        span: Span,
    ) -> Option<GenericParam> {
        if !self.is_unbound(var) {
            return None;
        }
        let default = match self.unifier.literal_kind(var) {
            Some(LiteralKind::Float) => Some(PrimitiveType::F64),
            Some(LiteralKind::Integer) => Some(PrimitiveType::I64),
            None => None,
        };
        if let Some(primitive) = default {
            let defaulted = self
                .unifier
                .unify(&Type::Var(var), &Type::Primitive(primitive), span);
            debug_assert!(
                defaulted.is_ok(),
                "a literal variable accepts its default type"
            );
            return None;
        }
        let name = Self::parameter_name(position);
        let bounds = self.self_plain_bounds(var);
        let param = self.new_generic(&name, bounds);
        self.unifier.bind(var, Type::Generic(param.id));
        Some(param)
    }

    fn self_plain_bounds(&self, var: TypeVar) -> Vec<Bound> {
        let mut bounds = Vec::new();
        for bound in self.unifier.bounds(var) {
            let bound = match &bound.argument {
                Some(argument) => {
                    let argument = self.unifier.resolve(argument);
                    if argument == Type::Var(var) {
                        Bound::plain(bound.binding)
                    } else {
                        Bound {
                            binding: bound.binding,
                            argument: Some(argument),
                        }
                    }
                }
                None => bound.clone(),
            };
            if !bounds.contains(&bound) {
                bounds.push(bound);
            }
        }
        bounds
    }

    fn monomorphic_vars(&self) -> HashSet<TypeVar> {
        self.monomorphic
            .iter()
            .flat_map(|monomorphic| self.free_vars(monomorphic))
            .collect()
    }

    pub(super) fn free_vars(&self, value: &Type) -> Vec<TypeVar> {
        let mut vars = Vec::new();
        Self::collect_vars(&self.unifier.resolve(value), &mut vars);
        vars
    }

    fn collect_vars(value: &Type, vars: &mut Vec<TypeVar>) {
        match value {
            Type::Var(var) if !vars.contains(var) => vars.push(*var),
            Type::Function(_, parameters, result) => {
                parameters
                    .iter()
                    .for_each(|parameter| Self::collect_vars(parameter, vars));
                Self::collect_vars(result, vars);
            }
            Type::Named(_, arguments) => arguments
                .iter()
                .for_each(|argument| Self::collect_vars(argument, vars)),
            Type::Ref(_, inner) | Type::Mut(_, inner) => Self::collect_vars(inner, vars),
            _ => {}
        }
    }

    fn failed_scheme(&mut self) -> Scheme {
        let param = self.new_generic(Self::parameter_name(0).as_str(), Vec::new());
        Scheme {
            lifetimes: Vec::new(),
            body: Type::Generic(param.id),
            params: vec![param],
        }
    }

    pub(super) fn parameter_name(position: usize) -> String {
        let letters: Vec<char> = Self::PARAMETER_NAMES.chars().collect();
        match letters.get(position) {
            Some(letter) => letter.to_string(),
            None => format!("t{position}"),
        }
    }

    fn collect_struct(
        &mut self,
        binding: BindingId,
        binders: &[TypeBinder],
        fields: &[(crate::core::FieldLabel, TypeExpr)],
    ) -> Result<(), Diagnostic> {
        let params = self.struct_params(binders)?;
        let lifetimes = Self::lifetime_binders(binders);
        let scope: HashMap<BindingId, GenericId> = binders
            .iter()
            .filter(|binder| !binder.lifetime)
            .zip(&params)
            .map(|(binder, param)| (binder.variable, param.id))
            .collect();
        let (lowered, field_spans) = self.lower_fields(fields, &scope)?;
        Self::check_parameters_used(binders, &params, &lowered)?;
        self.structs.add(
            binding,
            StructDefinition {
                lifetimes,
                params,
                fields: lowered,
                field_spans,
                derives: Vec::new(),
                derived_traits: Vec::new(),
                blocked: Vec::new(),
                written_derive: None,
            },
        );
        Ok(())
    }

    fn collect_enum(
        &mut self,
        binding: BindingId,
        binders: &[TypeBinder],
        variants: &[VariantDeclaration],
        rust: &Option<crate::core::EnumRust>,
    ) -> Result<(), Diagnostic> {
        let params = self.struct_params(binders)?;
        let lifetimes = Self::lifetime_binders(binders);
        let scope: HashMap<BindingId, GenericId> = binders
            .iter()
            .filter(|binder| !binder.lifetime)
            .zip(&params)
            .map(|(binder, param)| (binder.variable, param.id))
            .collect();
        let mut lowered = Vec::new();
        for variant in variants {
            let (payload, payload_spans) = self.lower_variant_payload(&variant.payload, &scope)?;
            lowered.push(VariantDefinition {
                name: variant.name.clone(),
                payload,
                payload_spans,
                span: variant.span,
            });
        }
        self.check_enum_parameters_used(binding, binders, &params, variants)?;
        if let Some(rust) = rust {
            self.rust_types.add(
                binding,
                RustType {
                    template: rust.template.clone(),
                    clauses: Vec::new(),
                },
            );
        }
        self.enums.add(
            binding,
            EnumDefinition {
                lifetimes,
                params,
                variants: lowered,
                rust: rust.clone(),
                derives: Vec::new(),
                derived_traits: Vec::new(),
                blocked: Vec::new(),
                written_derive: None,
            },
        );
        Ok(())
    }

    fn lower_variant_payload(
        &mut self,
        payload: &CoreVariantPayload,
        scope: &HashMap<BindingId, GenericId>,
    ) -> Result<(VariantPayload, Vec<Span>), Diagnostic> {
        match payload {
            CoreVariantPayload::None => Ok((VariantPayload::None, Vec::new())),
            CoreVariantPayload::Positional(types) => Ok((
                VariantPayload::Positional(
                    types
                        .iter()
                        .map(|payload| self.lower_type(payload, scope))
                        .collect::<Result<Vec<_>, Diagnostic>>()?,
                ),
                types.iter().map(TypeExpr::span).collect(),
            )),
            CoreVariantPayload::Named(fields) => Ok((
                VariantPayload::Named(
                    fields
                        .iter()
                        .map(|(label, field_type)| {
                            Ok((label.name.clone(), self.lower_type(field_type, scope)?))
                        })
                        .collect::<Result<Vec<_>, Diagnostic>>()?,
                ),
                fields
                    .iter()
                    .map(|(_, field_type)| field_type.span())
                    .collect(),
            )),
        }
    }

    fn check_enum_parameters_used(
        &self,
        enum_binding: BindingId,
        binders: &[TypeBinder],
        params: &[GenericParam],
        variants: &[VariantDeclaration],
    ) -> Result<(), Diagnostic> {
        let enum_name = self.bindings.get(enum_binding).name();
        let problem = binders
            .iter()
            .filter(|binder| !binder.lifetime)
            .zip(params)
            .find_map(|(binder, param)| {
                let usage = ParameterUsage::scan(variants, enum_binding, binder.variable);
                match (usage.outside_self, usage.first_recursive) {
                    (true, _) => None,
                    (false, Some(recursive_use)) => {
                        Some(TypeProblems::recursively_used_enum_parameter(
                            &param.name,
                            enum_name,
                            binder.span,
                            recursive_use,
                        ))
                    }
                    (false, None) => Some(TypeProblems::unused_enum_parameter(
                        &param.name,
                        binder.span,
                    )),
                }
            });
        problem.map_or(Ok(()), Err)
    }

    fn lower_fields(
        &mut self,
        fields: &[(crate::core::FieldLabel, TypeExpr)],
        scope: &HashMap<BindingId, GenericId>,
    ) -> Result<(LoweredFields, Vec<Span>), Diagnostic> {
        let mut lowered = Vec::new();
        let mut spans = Vec::new();
        for (label, field_type) in fields {
            lowered.push((label.name.clone(), self.lower_type(field_type, scope)?));
            spans.push(field_type.span());
        }
        Ok((lowered, spans))
    }

    fn collect_rust_type(
        &mut self,
        binding: BindingId,
        binders: &[TypeBinder],
        template: &Rc<ParsedTemplate>,
        clauses: &[(TypeExpr, Rc<ParsedTemplate>)],
    ) -> Result<(), Diagnostic> {
        self.reject_bounds(binders)?;
        let clauses = clauses
            .iter()
            .map(|(target, template)| {
                Ok((self.lower_type(target, &HashMap::new())?, template.clone()))
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        self.rust_types.add(
            binding,
            RustType {
                template: template.clone(),
                clauses,
            },
        );
        Ok(())
    }

    fn struct_params(&mut self, binders: &[TypeBinder]) -> Result<Vec<GenericParam>, Diagnostic> {
        self.reject_bounds(binders)?;
        Ok(binders
            .iter()
            .filter(|binder| !binder.lifetime)
            .map(|binder| {
                let name = self.bindings.get(binder.variable).name().to_string();
                self.new_generic(&name, Vec::new())
            })
            .collect())
    }

    fn lifetime_binders(binders: &[TypeBinder]) -> Vec<BindingId> {
        binders
            .iter()
            .filter(|binder| binder.lifetime)
            .map(|binder| binder.variable)
            .collect()
    }

    fn reject_bounds(&self, binders: &[TypeBinder]) -> Result<(), Diagnostic> {
        match binders.iter().find(|binder| !binder.bounds.is_empty()) {
            Some(binder) => Err(TypeProblems::bounded_type_parameter(
                self.bindings.get(binder.variable).name(),
                binder.span,
            )),
            None => Ok(()),
        }
    }

    fn check_parameters_used(
        binders: &[TypeBinder],
        params: &[GenericParam],
        fields: &[(std::rc::Rc<str>, Type)],
    ) -> Result<(), Diagnostic> {
        let unused = binders
            .iter()
            .filter(|binder| !binder.lifetime)
            .zip(params)
            .find(|(_, param)| {
                !fields
                    .iter()
                    .any(|(_, field_type)| field_type.mentions(param.id))
            });
        match unused {
            Some((binder, param)) => Err(TypeProblems::unused_type_parameter(
                &param.name,
                binder.span,
            )),
            None => Ok(()),
        }
    }

    fn collect_user_trait(
        &mut self,
        module: ModuleId,
        declaration: UserTrait<'_>,
    ) -> Result<(), Diagnostic> {
        let UserTrait {
            binding,
            rust_name,
            self_variable,
            supertraits,
            associated,
            methods,
        } = declaration;
        let associated: Vec<super::typed::TypedAssociatedType> = associated
            .iter()
            .map(|associated| super::typed::TypedAssociatedType {
                binding: associated.binding,
                name: associated.name.clone(),
            })
            .collect();
        if !associated.is_empty() {
            self.trait_arguments.insert(binding);
        }
        let supertraits = supertraits
            .iter()
            .map(|bound| self.lower_type_bound(bound, &HashMap::new()))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        self.traits.add(
            binding,
            RustTrait {
                path: rust_name.clone(),
                implementors: Vec::new(),
                help: None,
                supertraits: supertraits.clone(),
                associated: associated.first().map(|associated| associated.name.clone()),
            },
        );
        let mut typed_methods = Vec::new();
        for method in methods {
            let receiver = Self::receiver_kind(&method.signature, self_variable);
            self.synthetic_bindings.insert(method.binding);
            if let Some(default) = method.default {
                self.synthetic_bindings.insert(default);
            }
            self.trait_methods.insert(
                method.binding,
                TraitMethodInfo {
                    trait_binding: binding,
                    rust_name: rust_name.clone(),
                    receiver,
                },
            );
            typed_methods.push(TypedTraitMethod {
                binding: method.binding,
                receiver,
                default: method.default,
            });
        }
        self.trait_definitions.push(TypedTrait {
            binding,
            module,
            rust_name: rust_name.clone(),
            supertraits,
            associated,
            methods: typed_methods,
        });
        Ok(())
    }

    fn receiver_kind(signature: &TypeExpr, self_variable: BindingId) -> Option<ReceiverKind> {
        let TypeExprKind::Function(_, parameters, _) = signature.kind() else {
            return None;
        };
        parameters
            .first()
            .and_then(|parameter| Self::parameter_receiver(parameter, self_variable))
    }

    fn parameter_receiver(parameter: &TypeExpr, self_variable: BindingId) -> Option<ReceiverKind> {
        match parameter.kind() {
            TypeExprKind::Named(binding, arguments) => {
                Self::named_receiver(*binding, arguments, self_variable, ReceiverKind::Owned)
            }
            TypeExprKind::Reference(kind, _, inner) => {
                Self::reference_receiver(*kind, inner, self_variable)
            }
            _ => None,
        }
    }

    fn reference_receiver(
        kind: ReferenceKind,
        inner: &TypeExpr,
        self_variable: BindingId,
    ) -> Option<ReceiverKind> {
        let TypeExprKind::Named(binding, arguments) = inner.kind() else {
            return None;
        };
        let receiver = match kind {
            ReferenceKind::Shared => ReceiverKind::Shared,
            ReferenceKind::Mutable => ReceiverKind::Mutable,
        };
        Self::named_receiver(*binding, arguments, self_variable, receiver)
    }

    fn named_receiver(
        binding: BindingId,
        arguments: &[TypeExpr],
        self_variable: BindingId,
        receiver: ReceiverKind,
    ) -> Option<ReceiverKind> {
        (binding == self_variable && arguments.is_empty()).then_some(receiver)
    }

    fn collect_trait_impls(&mut self, modules: &[CoreModule]) {
        for module in modules {
            for item in &module.items {
                let ItemKind::Impl(trait_binding, implementor, associated, methods) = &item.kind
                else {
                    continue;
                };
                let implementor_span = implementor.span();
                let (implementor, params, associated_values) =
                    match self.lower_impl_implementor(implementor, associated) {
                        Ok(implementor) => implementor,
                        Err(diagnostic) => {
                            self.diagnostics.push(diagnostic);
                            continue;
                        }
                    };
                if self.report_foreign_implementor(*trait_binding, &implementor, implementor_span) {
                    continue;
                }
                self.report_unsized_implementor(&implementor, implementor_span, methods);
                let argument = self
                    .traits
                    .get(*trait_binding)
                    .is_some_and(|rust_trait| rust_trait.associated.is_some())
                    .then(|| associated_values.first().map(|(_, value)| value.clone()))
                    .flatten();
                self.traits.add_implementor(
                    *trait_binding,
                    Implementor {
                        scheme: Scheme {
                            lifetimes: Vec::new(),
                            params: params.clone(),
                            body: implementor.clone(),
                        },
                        argument,
                    },
                );
                let mut typed_methods = Vec::new();
                for method in methods {
                    self.synthetic_bindings.insert(method.binding);
                    self.impl_methods.insert(method.binding);
                    let receiver = self
                        .trait_methods
                        .get(&method.trait_method)
                        .and_then(|info| info.receiver);
                    typed_methods.push(super::typed::TypedImplMethod {
                        binding: method.binding,
                        receiver,
                    });
                }
                let associated = associated_values
                    .into_iter()
                    .map(|(binding, value)| {
                        let name = self
                            .trait_definitions
                            .iter()
                            .find(|trait_definition| trait_definition.binding == *trait_binding)
                            .and_then(|trait_definition| {
                                trait_definition
                                    .associated
                                    .iter()
                                    .find(|associated| associated.binding == binding)
                            })
                            .map_or_else(|| Rc::from(""), |associated| associated.name.clone());
                        (binding, name, value)
                    })
                    .collect();
                self.trait_impls.push(TypedTraitImpl {
                    trait_binding: *trait_binding,
                    implementor,
                    params,
                    associated,
                    implementor_span,
                    module: module.id,
                    methods: typed_methods,
                });
            }
        }
    }

    fn is_boxed_trait(&self, implementor: &Type, trait_binding: BindingId) -> bool {
        let Some(box_binding) = self.box_binding else {
            return false;
        };
        matches!(
            implementor,
            Type::Named(binding, arguments)
                if *binding == box_binding
                    && matches!(arguments.as_slice(), [Type::TraitObject(object)] if *object == trait_binding)
        )
    }

    fn report_supertraits(&mut self) {
        let implementations: Vec<(BindingId, Type, Span)> = self
            .trait_impls
            .iter()
            .map(|implementation| {
                (
                    implementation.trait_binding,
                    implementation.implementor.clone(),
                    implementation.implementor_span,
                )
            })
            .collect();
        for (trait_binding, implementor, span) in implementations {
            self.report_missing_supertraits(trait_binding, &implementor, span);
        }
    }

    fn report_missing_supertraits(
        &mut self,
        trait_binding: BindingId,
        implementor: &Type,
        span: Span,
    ) {
        let Some(rust_trait) = self.traits.get(trait_binding) else {
            return;
        };
        let supertraits = rust_trait.supertraits.clone();
        if supertraits.is_empty() {
            return;
        }
        let trait_name = self.bindings.get(trait_binding).name().to_string();
        let implementor_name = self.display().show(implementor);
        for supertrait in supertraits {
            if self
                .traits
                .satisfies(&supertrait, implementor, &|generic, bound| {
                    self.unifier.generic_bounds(generic).contains(bound)
                })
            {
                continue;
            }
            let supertrait_name = self.bindings.get(supertrait.binding).name().to_string();
            let help = self
                .traits
                .get(supertrait.binding)
                .and_then(|known| known.help.clone());
            self.diagnostics.push(TypeProblems::missing_supertrait(
                &trait_name,
                &supertrait_name,
                &implementor_name,
                span,
                help,
            ));
        }
    }

    fn lower_impl_implementor(
        &mut self,
        implementor: &TypeExpr,
        associated: &[crate::core::ImplAssociatedType],
    ) -> Result<LoweredImplementor, Diagnostic> {
        let mut params = Vec::new();
        let mut scope = HashMap::new();
        for variable in self.implementor_variables(implementor) {
            let name = self.bindings.get(variable).name().to_string();
            let param = self.new_generic(&name, Vec::new());
            scope.insert(variable, param.id);
            params.push(param);
        }
        let body = self.lower_type(implementor, &scope)?;
        let values = associated
            .iter()
            .map(|associated| {
                Ok((
                    associated.trait_type,
                    self.lower_type(&associated.value, &scope)?,
                ))
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        Ok((body, params, values))
    }

    fn implementor_variables(&self, implementor: &TypeExpr) -> Vec<BindingId> {
        let mut found = Vec::new();
        self.collect_implementor_variables(implementor, &mut found);
        found
    }

    fn collect_implementor_variables(&self, type_expr: &TypeExpr, found: &mut Vec<BindingId>) {
        if let TypeExprKind::Named(binding, arguments) = type_expr.kind()
            && arguments.is_empty()
            && matches!(
                self.bindings.get(*binding).kind(),
                BindingKind::TypeVariable
            )
            && !found.contains(binding)
        {
            found.push(*binding);
        }
        for part in type_expr.parts() {
            self.collect_implementor_variables(part, found);
        }
    }

    fn report_unsized_implementor(
        &mut self,
        implementor: &Type,
        span: Span,
        methods: &[crate::core::ImplMethod],
    ) {
        if !Self::is_unsized_implementor(&self.rust_types, implementor) {
            return;
        }
        for method in methods {
            let owned = self
                .trait_methods
                .get(&method.trait_method)
                .is_some_and(|info| info.receiver == Some(ReceiverKind::Owned));
            if !owned {
                continue;
            }
            let implementor_name = self.display().show(implementor);
            let method_name = self.bindings.get(method.trait_method).name().to_string();
            self.diagnostics.push(TypeProblems::unsized_implementor(
                &implementor_name,
                &method_name,
                span,
            ));
        }
    }

    fn report_foreign_implementor(
        &mut self,
        trait_binding: BindingId,
        implementor: &Type,
        span: Span,
    ) -> bool {
        if !self.rust_trait_paths.contains_key(&trait_binding) || self.is_program_type(implementor)
        {
            return false;
        }
        let implementor_name = self.display().show(implementor);
        let trait_name = self.bindings.get(trait_binding).name().to_string();
        self.diagnostics.push(TypeProblems::foreign_implementor(
            &trait_name,
            &implementor_name,
            span,
        ));
        true
    }

    fn is_program_type(&self, implementor: &Type) -> bool {
        matches!(
            implementor,
            Type::Named(binding, _)
                if self.structs.get(*binding).is_some() || self.enums.get(*binding).is_some()
        )
    }

    fn is_unsized_implementor(rust_types: &RustTypeTable, implementor: &Type) -> bool {
        match implementor {
            Type::Primitive(PrimitiveType::Str) => true,
            Type::Named(binding, _) => rust_types.is_slice(*binding),
            _ => false,
        }
    }

    fn collect_trait(
        &mut self,
        binding: BindingId,
        path: &std::rc::Rc<str>,
        implementors: &[TypeExpr],
        help: Option<Rc<str>>,
    ) -> Result<(), Diagnostic> {
        let trait_name = self.bindings.get(binding).name().to_string();
        let implementors = implementors
            .iter()
            .map(|implementor| self.lower_implementor(implementor, &trait_name))
            .collect::<Result<Vec<Implementor>, Diagnostic>>()?;
        self.traits.add(
            binding,
            RustTrait {
                path: path.clone(),
                implementors,
                help,
                supertraits: Vec::new(),
                associated: None,
            },
        );
        Ok(())
    }

    fn collect_rust_trait(
        &mut self,
        declaration: &crate::core::RustTraitDeclaration,
    ) -> Result<(), Diagnostic> {
        self.collect_trait(
            declaration.binding,
            &declaration.path,
            &declaration.implementors,
            declaration.help.clone(),
        )?;
        self.rust_trait_paths
            .insert(declaration.binding, Rc::clone(&declaration.path));
        for method in &declaration.methods {
            let receiver = Self::receiver_kind(&method.signature, declaration.self_variable);
            self.synthetic_bindings.insert(method.binding);
            self.trait_methods.insert(
                method.binding,
                TraitMethodInfo {
                    trait_binding: declaration.binding,
                    rust_name: Rc::clone(&declaration.path),
                    receiver,
                },
            );
        }
        Ok(())
    }

    fn absorb_impl_bounds(&mut self, definitions: &HashMap<BindingId, TypedDefinition>) {
        for position in 0..self.trait_impls.len() {
            let params = self.trait_impls[position].params.clone();
            let mut found: Vec<Vec<Bound>> = vec![Vec::new(); params.len()];
            for method in &self.trait_impls[position].methods {
                let definition = definitions
                    .get(&method.binding)
                    .expect("an impl method has a definition");
                let replacements: HashMap<GenericId, Type> = definition
                    .scheme
                    .params
                    .iter()
                    .zip(&params)
                    .map(|(method_param, implementation)| {
                        (method_param.id, Type::Generic(implementation.id))
                    })
                    .collect();
                for (bounds, method_param) in found.iter_mut().zip(&definition.scheme.params) {
                    let declared = method_param.bounds.iter();
                    let implied = self
                        .implied_bounds
                        .get(&method_param.id)
                        .into_iter()
                        .flatten();
                    for bound in declared.chain(implied) {
                        let bound = bound.substituted(&replacements);
                        if !bounds.contains(&bound) {
                            bounds.push(bound);
                        }
                    }
                }
            }
            self.store_impl_bounds(position, found);
        }
    }

    fn store_impl_bounds(&mut self, position: usize, bounds: Vec<Vec<Bound>>) {
        let mut params = self.trait_impls[position].params.clone();
        for (param, bounds) in params.iter_mut().zip(bounds) {
            param.bounds = bounds;
        }
        for param in &params {
            self.generics.insert(param.id, param.clone());
            self.unifier
                .register_generic(param.id, param.bounds.clone());
        }
        let trait_binding = self.trait_impls[position].trait_binding;
        let implementor = self.trait_impls[position].implementor.clone();
        self.trait_impls[position].params = params.clone();
        self.traits.set_implementor_scheme(
            trait_binding,
            &implementor,
            Scheme {
                lifetimes: Vec::new(),
                params,
                body: implementor.clone(),
            },
        );
    }

    fn check_generic_bound(&mut self, generic: GenericId, bound: Bound, span: Span) {
        if self.unifier.generic_bounds(generic).contains(&bound) {
            return;
        }
        if self.impl_generics.contains(&generic) {
            let implied = self.implied_bounds.entry(generic).or_default();
            if !implied.contains(&bound) {
                implied.push(bound);
            }
            return;
        }
        if self.implies(generic, &bound) {
            if self.declared_bound_implies(generic, &bound) {
                return;
            }
            let implied = self.implied_bounds.entry(generic).or_default();
            if !implied.contains(&bound) {
                implied.push(bound);
            }
            return;
        }
        let generic_name = self.display().show(&Type::Generic(generic));
        let diagnostic = TypeProblems::missing_bound(
            self.bindings.get(bound.binding).name(),
            &generic_name,
            span,
        );
        self.diagnostics.push(diagnostic);
    }

    fn implies(&self, generic: GenericId, bound: &Bound) -> bool {
        if self.declared_bound_implies(generic, bound) {
            return true;
        }
        if bound.argument.is_some() {
            return false;
        }
        match self.admissible_types(generic) {
            Admissible::Closed(types) => {
                !types.is_empty()
                    && types
                        .iter()
                        .all(|candidate| self.traits.satisfies(bound, candidate, &|_, _| false))
            }
            Admissible::Unbounded | Admissible::Open(_) => false,
        }
    }

    fn declared_bound_implies(&self, generic: GenericId, bound: &Bound) -> bool {
        self.unifier
            .generic_bounds(generic)
            .iter()
            .any(|declared| self.bound_implies(declared, bound))
    }

    fn bound_implies(&self, declared: &Bound, required: &Bound) -> bool {
        if declared == required {
            return true;
        }
        let Some(rust_trait) = self.traits.get(declared.binding) else {
            return false;
        };
        rust_trait
            .supertraits
            .iter()
            .any(|supertrait| self.bound_implies(supertrait, required))
    }

    pub(super) fn report_missing_trait(&mut self, bound: &Bound, implementor: &Type, span: Span) {
        let display = self.display();
        let mut diagnostic = self.missing_trait_diagnostic(bound, implementor, span, &display);
        if let Type::Named(binding, _) = implementor {
            diagnostic = self.type_trait_help(diagnostic, *binding, bound, &display);
        }
        self.diagnostics.push(diagnostic);
    }

    fn missing_trait_diagnostic(
        &self,
        bound: &Bound,
        implementor: &Type,
        span: Span,
        display: &TypeDisplay<'_>,
    ) -> Diagnostic {
        let trait_name = self.bindings.get(bound.binding).name();
        let found = display.show(implementor);
        let diagnostic = match implementor {
            Type::Named(binding, _)
                if self.structs.get(*binding).is_some() || self.enums.get(*binding).is_some() =>
            {
                TypeProblems::missing_trait_on_struct(trait_name, &found, span)
            }
            _ => {
                let implementors = self.implementor_names(bound.binding, display);
                TypeProblems::missing_trait(trait_name, &found, span, implementors)
            }
        };
        match self
            .traits
            .get(bound.binding)
            .and_then(|known| known.help.as_deref())
        {
            Some(help) => diagnostic.with_help(help),
            None => diagnostic,
        }
    }

    fn implementor_names(
        &self,
        trait_binding: BindingId,
        display: &TypeDisplay<'_>,
    ) -> Vec<String> {
        self.traits
            .get(trait_binding)
            .map(|rust_trait| {
                rust_trait
                    .implementors
                    .iter()
                    .map(|known| Self::show_implementor(display, known))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn type_trait_help(
        &self,
        diagnostic: Diagnostic,
        binding: BindingId,
        bound: &Bound,
        display: &TypeDisplay<'_>,
    ) -> Diagnostic {
        if self.enums.get(binding).is_some() {
            return self.enum_trait_help(diagnostic, binding, bound, display);
        }
        self.struct_trait_help(diagnostic, binding, bound, display)
    }

    fn enum_trait_help(
        &self,
        diagnostic: Diagnostic,
        binding: BindingId,
        bound: &Bound,
        display: &TypeDisplay<'_>,
    ) -> Diagnostic {
        let definition = self
            .enums
            .get(binding)
            .expect("enum trait help names an enum");
        let enum_name = self.bindings.get(binding).name().to_string();
        let trait_name = self.bindings.get(bound.binding).name();
        let mut diagnostic = match definition.written_derive {
            Some(span) if self.is_derivable(bound.binding) => diagnostic
                .with_label(TypeProblems::derive_leaves_out(span, trait_name))
                .with_help(TypeProblems::add_to_enum_derive_help(trait_name)),
            _ => diagnostic,
        };
        if self.is_order(bound.binding) {
            if definition.written_derive.is_none() {
                diagnostic =
                    diagnostic.with_note(TypeProblems::order_not_inferred_enum_note(trait_name));
            }
            if let Some((variant, field_type)) = self.blocking_enum_payload(binding, bound.binding)
            {
                diagnostic = diagnostic.with_note(TypeProblems::order_blocked_enum_note(
                    &enum_name,
                    &variant,
                    &display.show(&field_type),
                    trait_name,
                ));
            } else {
                let mut derives: Vec<String> = definition
                    .derived_traits
                    .iter()
                    .map(|binding| self.bindings.get(*binding).name().to_string())
                    .collect();
                derives.push(trait_name.to_string());
                let variants: Vec<Rc<str>> = definition
                    .variants
                    .iter()
                    .map(|variant| variant.name.clone())
                    .collect();
                diagnostic = diagnostic.with_help(TypeProblems::enum_order_help(
                    &enum_name, &derives, &variants,
                ));
            }
        } else if let Some((blocked, variant, payload)) = definition
            .blocked
            .iter()
            .find(|(blocked, _, _)| *blocked == bound.binding)
        {
            diagnostic = diagnostic.with_note(TypeProblems::blocked_enum_derive_note(
                &enum_name,
                self.bindings.get(*blocked).name(),
                variant,
                &display.show(payload),
            ));
        }
        diagnostic
    }

    fn struct_trait_help(
        &self,
        diagnostic: Diagnostic,
        struct_binding: BindingId,
        bound: &Bound,
        display: &TypeDisplay<'_>,
    ) -> Diagnostic {
        let Some(definition) = self.structs.get(struct_binding) else {
            return diagnostic;
        };
        let struct_name = self.bindings.get(struct_binding).name().to_string();
        if self.is_order(bound.binding) {
            self.with_order_help(
                diagnostic,
                definition,
                struct_binding,
                &struct_name,
                bound.binding,
                display,
            )
        } else {
            self.with_non_order_help(diagnostic, definition, bound.binding, &struct_name, display)
        }
    }

    fn with_non_order_help(
        &self,
        diagnostic: Diagnostic,
        definition: &StructDefinition,
        trait_binding: BindingId,
        struct_name: &str,
        display: &TypeDisplay<'_>,
    ) -> Diagnostic {
        let diagnostic = self.with_written_derive(diagnostic, definition, trait_binding);
        match definition
            .blocked
            .iter()
            .find(|(blocked, _, _)| *blocked == trait_binding)
        {
            Some((_, field, field_type)) => {
                diagnostic.with_note(TypeProblems::blocked_derive_note(
                    struct_name,
                    self.bindings.get(trait_binding).name(),
                    field,
                    &display.show(field_type),
                ))
            }
            None => diagnostic,
        }
    }

    fn is_order(&self, trait_binding: BindingId) -> bool {
        self.traits
            .get(trait_binding)
            .is_some_and(|rust_trait| rust_trait.path.as_ref() == crate::core::OrderTrait::PATH)
    }

    fn with_order_help(
        &self,
        diagnostic: Diagnostic,
        definition: &StructDefinition,
        struct_binding: BindingId,
        struct_name: &str,
        trait_binding: BindingId,
        display: &TypeDisplay<'_>,
    ) -> Diagnostic {
        let trait_name = self.bindings.get(trait_binding).name();
        let diagnostic = match definition.written_derive {
            Some(span) => diagnostic.with_label(TypeProblems::derive_leaves_out(span, trait_name)),
            None => diagnostic.with_note(TypeProblems::order_not_inferred_note(trait_name)),
        };
        let sortable = self.first_ord_field(definition, trait_binding);
        match self.blocking_field(struct_binding, trait_binding) {
            Some((field, field_type)) => diagnostic
                .with_note(TypeProblems::order_blocked_note(
                    struct_name,
                    &field,
                    &display.show(&field_type),
                    trait_name,
                ))
                .with_help(TypeProblems::sort_by_key_help(struct_name, sortable)),
            None => {
                let fields: Vec<Rc<str>> = definition
                    .fields
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect();
                let mut derives: Vec<String> = definition
                    .derived_traits
                    .iter()
                    .map(|binding| self.bindings.get(*binding).name().to_string())
                    .collect();
                derives.push(self.bindings.get(trait_binding).name().to_string());
                diagnostic
                    .with_help(TypeProblems::order_derive_help(
                        struct_name,
                        &derives,
                        &fields,
                    ))
                    .with_help(TypeProblems::sort_by_key_help(struct_name, sortable))
            }
        }
    }

    fn first_ord_field<'a>(
        &self,
        definition: &'a StructDefinition,
        trait_binding: BindingId,
    ) -> Option<&'a str> {
        let params: Vec<GenericId> = definition.params.iter().map(|param| param.id).collect();
        definition
            .fields
            .iter()
            .find(|(_, ty)| {
                self.traits
                    .satisfies(&Bound::plain(trait_binding), ty, &|generic, _| {
                        params.contains(&generic)
                    })
            })
            .map(|(name, _)| name.as_ref())
    }

    fn show_implementor(display: &TypeDisplay<'_>, implementor: &Implementor) -> String {
        let body = display.scheme(&implementor.scheme);
        match &implementor.argument {
            Some(argument) => format!("(with {body} {})", display.show(argument)),
            None => body,
        }
    }

    fn with_written_derive(
        &self,
        diagnostic: Diagnostic,
        definition: &StructDefinition,
        bound: BindingId,
    ) -> Diagnostic {
        let Some(derive_span) = definition.written_derive else {
            return diagnostic;
        };
        if !self.is_derivable(bound) {
            return diagnostic;
        }
        let trait_name = self.bindings.get(bound).name();
        diagnostic
            .with_label(TypeProblems::derive_leaves_out(derive_span, trait_name))
            .with_help(TypeProblems::add_to_derive_help(trait_name))
    }

    fn is_derivable(&self, bound: BindingId) -> bool {
        self.traits.get(bound).is_some_and(|rust_trait| {
            let name = StructDerives::derive_name(&rust_trait.path);
            StructDerives::DERIVABLE.contains(&name)
        })
    }
}

impl TypeChecker {
    pub fn check(
        program: &crate::expand::Program,
        changed_structs: &HashSet<BindingId>,
        inferred_copies: &InferredCopies,
        pinned: &HashMap<BindingId, Type>,
    ) -> Result<super::typed::TypedProgram, Diagnostics> {
        let mut checker = Self::new(program.bindings.clone());
        checker.changed_structs = changed_structs.clone();
        checker.inferred_copies = inferred_copies.clone();
        checker.pinned = pinned.clone();
        checker.macro_origins = program
            .macro_origins
            .iter()
            .map(|(span, origin)| (*span, Rc::clone(&origin.name)))
            .collect();
        checker.collect_declarations(&program.modules);
        let mut definitions = checker.check_definitions(&program.modules, program.entry);
        let order = Self::definition_order(&program.modules);
        let mut finalizer = super::finalize::Finalizer::new(&mut checker);
        for binding in &order {
            if let Some(definition) = definitions.get_mut(binding) {
                finalizer.definition(definition);
            }
        }
        checker.absorb_impl_bounds(&definitions);
        checker.discharge_obligations(ObligationPass::Final);
        checker.resolve_missing_annotations();
        let typed_modules = program
            .modules
            .iter()
            .map(|module| Self::typed_module(module, &mut definitions))
            .collect();
        checker.into_program(typed_modules, program.entry)
    }

    fn definition_order(modules: &[CoreModule]) -> Vec<BindingId> {
        modules
            .iter()
            .flat_map(|module| &module.items)
            .filter_map(|item| match &item.kind {
                ItemKind::Def(binding, _) => Some(*binding),
                _ => None,
            })
            .collect()
    }

    fn typed_module(
        module: &CoreModule,
        definitions: &mut HashMap<BindingId, TypedDefinition>,
    ) -> super::typed::TypedModule {
        let mut typed = super::typed::TypedModule {
            id: module.id,
            path: module.path.clone(),
            definitions: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            exports: module.exports.clone(),
        };
        for item in &module.items {
            match &item.kind {
                ItemKind::Def(binding, _) => typed.definitions.extend(definitions.remove(binding)),
                ItemKind::Struct(binding, ..) => typed.structs.push(*binding),
                ItemKind::Enum(binding, ..) => typed.enums.push(*binding),
                ItemKind::Signature(..)
                | ItemKind::RustTrait(..)
                | ItemKind::RustType(..)
                | ItemKind::Trait(..)
                | ItemKind::Impl(..)
                | ItemKind::Derive(..) => {}
            }
        }
        typed
    }

    fn into_program(
        self,
        modules: Vec<super::typed::TypedModule>,
        entry: crate::core::ModuleId,
    ) -> Result<super::typed::TypedProgram, Diagnostics> {
        let warnings = self.warnings;
        let mut diagnostics = self.diagnostics;
        if diagnostics.has_errors() {
            diagnostics.extend(warnings);
            return Err(diagnostics);
        }
        Ok(super::typed::TypedProgram {
            modules,
            entry,
            bindings: self.bindings,
            structs: self.structs,
            enums: self.enums,
            rust_types: self.rust_types,
            box_binding: self.box_binding,
            traits: self.traits,
            trait_definitions: self.trait_definitions,
            rust_trait_paths: self.rust_trait_paths,
            trait_impls: self.trait_impls,
            associated_generics: self.associated_generics,
            conversions: self.conversions,
            trait_methods: self.trait_methods,
            synthetic_bindings: self.synthetic_bindings,
            generics: self.generics,
            implied_bounds: self.implied_bounds,
            template_borrows: self.template_borrows,
            pending_mode_checks: self.pending_mode_checks,
            borrowed_parameters: HashSet::new(),
            mutable_function_bindings: HashSet::new(),
            warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::TypeChecker;
    use crate::core::{BindingArena, BindingId, BindingInfo, BindingKind};
    use crate::source::{SourceId, Span};
    use crate::types::{Bound, Type, TypeVar};

    struct LinkingFixture {
        checker: TypeChecker,
        span: Span,
    }

    impl LinkingFixture {
        fn new() -> Self {
            Self {
                checker: TypeChecker::new(BindingArena::default()),
                span: Span::new(SourceId::new(0), 0..0),
            }
        }

        fn fresh(&mut self) -> TypeVar {
            match self.checker.unifier.fresh() {
                Type::Var(var) => var,
                other => panic!("a fresh type is a variable, found {other:?}"),
            }
        }

        fn bound_trait(&mut self) -> BindingId {
            self.checker.bindings.add(BindingInfo::new(
                "trait".into(),
                BindingKind::TypeVariable,
                None,
            ))
        }

        fn link(&mut self, var: TypeVar, other: TypeVar) -> bool {
            let binding = self.bound_trait();
            self.checker.unifier.add_bound(
                var,
                Bound {
                    binding,
                    argument: Some(Type::Var(other)),
                },
            );
            self.checker.link_var(var, &[var, other], self.span)
        }
    }

    #[test]
    fn parameters_past_the_alphabet_are_numbered() {
        assert_eq!(TypeChecker::parameter_name(0), "a");
        assert_eq!(TypeChecker::parameter_name(26), "t26");
    }

    #[test]
    fn a_two_type_bound_links_its_variables() {
        let mut fixture = LinkingFixture::new();
        let var = fixture.fresh();
        let other = fixture.fresh();
        assert!(fixture.link(var, other));
        assert_eq!(
            fixture.checker.unifier.shallow(&Type::Var(var)),
            Type::Var(other)
        );
    }
}
