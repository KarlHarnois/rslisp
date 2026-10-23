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
use super::typed::{PendingModeCheck, TypedDefinition, TypedExpr, TypedExprKind, TypedLeaf};
use super::unifier::{LiteralKind, Obligation, Unifier};
use crate::core::{
    BindingArena, BindingId, BodyItem, CopyTrait, CoreModule, Expr, ExprKind, Item, ItemKind, Leaf,
    ModuleId, ParsedTemplate, PrimitiveType, StructDerives, StructExpr, TypeBinder, TypeExpr,
    TypeExprKind, VariantDeclaration, VariantPayload as CoreVariantPayload,
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
    pub(super) diagnostics: Diagnostics,
    pub(super) warnings: Diagnostics,
    missing_annotations: Diagnostics,
    pub(super) type_arities: HashMap<BindingId, usize>,
    trait_arguments: HashSet<BindingId>,
    signatures: HashMap<BindingId, (TypeExpr, Span)>,
    monomorphic: Vec<Type>,
    implied_bounds: HashMap<GenericId, Vec<Bound>>,
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
}

struct Definition<'p> {
    binding: BindingId,
    value: &'p Expr,
    span: Span,
}

type LoweredFields = Vec<(Rc<str>, Type)>;

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
            diagnostics: Diagnostics::default(),
            warnings: Diagnostics::default(),
            missing_annotations: Diagnostics::default(),
            type_arities: HashMap::new(),
            trait_arguments: HashSet::new(),
            signatures: HashMap::new(),
            monomorphic: Vec::new(),
            implied_bounds: HashMap::new(),
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
        }
    }

    pub(super) fn collect_declarations(&mut self, modules: &[CoreModule]) {
        self.record_type_arities(modules);
        self.record_trait_arguments(modules);
        for item in modules.iter().flat_map(|module| &module.items) {
            if let Err(diagnostic) = self.collect_item(item) {
                self.diagnostics.push(diagnostic);
            }
        }
        self.box_binding = self.rust_types.box_binding();
        if let Some(box_binding) = self.box_binding {
            super::boxes::BoxPlanner::add_boxes(&mut self.structs, &mut self.enums, box_binding);
        }
        self.register_derived_traits(modules);
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
            TypeExprKind::Function(parameters, _) => parameters
                .get(position)
                .is_none_or(|parameter| !matches!(parameter.kind(), TypeExprKind::Hole)),
            _ => true,
        }
    }

    fn collect_item(&mut self, item: &Item) -> Result<(), Diagnostic> {
        match &item.kind {
            ItemKind::Struct(binding, params, fields) => {
                self.collect_struct(*binding, params, fields)
            }
            ItemKind::Enum(binding, params, variants, rust) => {
                self.collect_enum(*binding, params, variants, rust)
            }
            ItemKind::RustTrait(binding, path, implementors) => {
                self.collect_trait(*binding, path, implementors)
            }
            ItemKind::RustType(binding, binders, template, clauses) => {
                self.collect_rust_type(*binding, binders, template, clauses)
            }
            ItemKind::Signature(binding, signature) => {
                self.signatures
                    .insert(*binding, (signature.clone(), item.span));
                Ok(())
            }
            ItemKind::Derive(..) => Ok(()),
            ItemKind::Def(binding, value) => {
                self.record_field_accessor(*binding, value);
                Ok(())
            }
        }
    }

    fn record_trait_arguments(&mut self, modules: &[CoreModule]) {
        for item in modules.iter().flat_map(|module| &module.items) {
            let ItemKind::RustTrait(binding, _, implementors) = &item.kind else {
                continue;
            };
            if implementors.iter().any(Self::entry_uses_with) {
                self.trait_arguments.insert(*binding);
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
            let ItemKind::RustTrait(trait_binding, path, implementors) = &item.kind else {
                continue;
            };
            for implementor in implementors {
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
                self.type_arities.insert(*binding, params.len());
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
        match self.traits.select(&bound, implementor) {
            Selection::Implemented(requirements) => {
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
            let scheme = self.finish_scheme(binding, &value, signatures.get(&binding));
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

    fn borrow_unsized_result(&mut self, binding: BindingId, value: &mut TypedExpr) {
        let Type::Function(parameters, result) = self.unifier.resolve(&value.ty) else {
            return;
        };
        if !result.is_unsized() {
            return;
        }
        let borrowed = Type::Function(parameters, Box::new(Type::Ref(result)));
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
            },
        ))
    }

    fn prepare_member(&mut self, member: &Definition) -> Option<(BindingId, Scheme)> {
        let Some((signature, _)) = self.signatures.get(&member.binding).cloned() else {
            let shape = self.unwritten_shape(member.value);
            self.component.insert(member.binding, shape);
            return None;
        };
        match self.lower_scheme(&signature) {
            Ok(scheme) => {
                self.component.insert(member.binding, scheme.body.clone());
                Some((member.binding, scheme))
            }
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                let fresh = self.unifier.fresh();
                self.component.insert(member.binding, fresh);
                None
            }
        }
    }

    fn unwritten_shape(&mut self, value: &Expr) -> Type {
        let ExprKind::Fn(lambda) = value.kind() else {
            return self.unifier.fresh();
        };
        let parameters = lambda.params.iter().map(|_| self.unifier.fresh()).collect();
        Type::Function(parameters, Box::new(self.unifier.fresh()))
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
                params: signature.params.clone(),
                body: inferred,
            };
        }
        if matches!(value.kind, TypedExprKind::Fn(_)) {
            let params = self.mentioned_generics(&inferred);
            return Scheme {
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
            Type::Function(parameters, result) => {
                parameters
                    .iter()
                    .for_each(|parameter| Self::collect_vars(parameter, vars));
                Self::collect_vars(result, vars);
            }
            Type::Named(_, arguments) => arguments
                .iter()
                .for_each(|argument| Self::collect_vars(argument, vars)),
            Type::Ref(inner) | Type::Mut(inner) => Self::collect_vars(inner, vars),
            _ => {}
        }
    }

    fn failed_scheme(&mut self) -> Scheme {
        let param = self.new_generic(Self::parameter_name(0).as_str(), Vec::new());
        Scheme {
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
        let scope: HashMap<BindingId, GenericId> = binders
            .iter()
            .zip(&params)
            .map(|(binder, param)| (binder.variable, param.id))
            .collect();
        let (lowered, borrowed) = self.lower_fields(fields, &scope)?;
        Self::check_parameters_used(binders, &params, &lowered)?;
        self.structs.add(
            binding,
            StructDefinition {
                params,
                fields: lowered,
                derives: Vec::new(),
                derived_traits: Vec::new(),
                blocked: Vec::new(),
                written_derive: None,
            },
        );
        borrowed.map_or(Ok(()), |span| {
            Err(TypeProblems::borrowed_struct_field(span))
        })
    }

    fn collect_enum(
        &mut self,
        binding: BindingId,
        binders: &[TypeBinder],
        variants: &[VariantDeclaration],
        rust: &Option<crate::core::EnumRust>,
    ) -> Result<(), Diagnostic> {
        let params = self.struct_params(binders)?;
        let scope: HashMap<BindingId, GenericId> = binders
            .iter()
            .zip(&params)
            .map(|(binder, param)| (binder.variable, param.id))
            .collect();
        let mut lowered = Vec::new();
        for variant in variants {
            let payload = self.lower_variant_payload(&variant.payload, &scope)?;
            lowered.push(VariantDefinition {
                name: variant.name.clone(),
                payload,
                span: variant.span,
            });
        }
        Self::check_enum_parameters_used(binders, &params, &lowered)?;
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
    ) -> Result<VariantPayload, Diagnostic> {
        match payload {
            CoreVariantPayload::None => Ok(VariantPayload::None),
            CoreVariantPayload::Positional(types) => Ok(VariantPayload::Positional(
                types
                    .iter()
                    .map(|payload| self.lower_type(payload, scope))
                    .collect::<Result<Vec<_>, Diagnostic>>()?,
            )),
            CoreVariantPayload::Named(fields) => Ok(VariantPayload::Named(
                fields
                    .iter()
                    .map(|(label, field_type)| {
                        Ok((label.name.clone(), self.lower_type(field_type, scope)?))
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?,
            )),
        }
    }

    fn check_enum_parameters_used(
        binders: &[TypeBinder],
        params: &[GenericParam],
        variants: &[VariantDefinition],
    ) -> Result<(), Diagnostic> {
        let unused = binders.iter().zip(params).find(|(_, param)| {
            !variants
                .iter()
                .flat_map(VariantDefinition::payload_types)
                .any(|payload| payload.mentions(param.id))
        });
        match unused {
            Some((binder, param)) => Err(TypeProblems::unused_enum_parameter(
                &param.name,
                binder.span,
            )),
            None => Ok(()),
        }
    }

    fn lower_fields(
        &mut self,
        fields: &[(crate::core::FieldLabel, TypeExpr)],
        scope: &HashMap<BindingId, GenericId>,
    ) -> Result<(LoweredFields, Option<Span>), Diagnostic> {
        let mut lowered = Vec::new();
        let mut borrowed = None;
        for (label, field_type) in fields {
            let lowered_type = self.lower_type(field_type, scope)?;
            if lowered_type.stores_reference() && borrowed.is_none() {
                borrowed = Some(field_type.span());
            }
            lowered.push((label.name.clone(), lowered_type));
        }
        Ok((lowered, borrowed))
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
            .map(|binder| {
                let name = self.bindings.get(binder.variable).name().to_string();
                self.new_generic(&name, Vec::new())
            })
            .collect())
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
        let unused = binders.iter().zip(params).find(|(_, param)| {
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

    fn collect_trait(
        &mut self,
        binding: BindingId,
        path: &std::rc::Rc<str>,
        implementors: &[TypeExpr],
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
            },
        );
        Ok(())
    }

    fn check_generic_bound(&mut self, generic: GenericId, bound: Bound, span: Span) {
        if self.unifier.generic_bounds(generic).contains(&bound) {
            return;
        }
        if self.implies(generic, &bound) {
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

    fn report_missing_trait(&mut self, bound: &Bound, implementor: &Type, span: Span) {
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
        match implementor {
            Type::Named(binding, _)
                if self.structs.get(*binding).is_some() || self.enums.get(*binding).is_some() =>
            {
                TypeProblems::missing_trait_on_struct(trait_name, &found, span)
            }
            _ => {
                let implementors = self.implementor_names(bound.binding, display);
                TypeProblems::missing_trait(trait_name, &found, span, implementors)
            }
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
        if self
            .traits
            .get(bound.binding)
            .is_some_and(|rust_trait| rust_trait.path.as_ref() == "std::fmt::Display")
        {
            return diagnostic.with_help(TypeProblems::print_enum_help());
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
        let diagnostic = if self.is_order(bound.binding) {
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
        };
        if self
            .traits
            .get(bound.binding)
            .is_some_and(|rust_trait| rust_trait.path.as_ref() == "std::fmt::Display")
        {
            return diagnostic.with_help(TypeProblems::print_struct_help());
        }
        diagnostic
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
    ) -> Result<super::typed::TypedProgram, Diagnostics> {
        let mut checker = Self::new(program.bindings.clone());
        checker.changed_structs = changed_structs.clone();
        checker.inferred_copies = inferred_copies.clone();
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
            generics: self.generics,
            implied_bounds: self.implied_bounds,
            template_borrows: self.template_borrows,
            pending_mode_checks: self.pending_mode_checks,
            borrowed_parameters: HashSet::new(),
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
