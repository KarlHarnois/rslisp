use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::display::TypeDisplay;
use super::graph::DependencyGraph;
use super::pending::PendingBorrow;
use super::problems::{MismatchContext, TypeProblems};
use super::rust_types::{RustType, RustTypeTable};
use super::selection::Selection;
use super::structs::{StructDefinition, StructTable};
use super::templates::TemplateBorrows;
use super::traits::{RustTrait, TraitTable};
use super::ty::{Bound, GenericId, GenericParam, Scheme, Type, TypeVar};
use super::typed::{TypedDefinition, TypedExpr, TypedExprKind, TypedLeaf};
use super::unifier::{LiteralKind, Obligation, Unifier};
use crate::core::{
    BindingArena, BindingId, BodyItem, CoreModule, Expr, ExprKind, Item, ItemKind, Leaf, ModuleId,
    ParsedTemplate, PrimitiveType, StructDerives, StructExpr, TypeBinder, TypeExpr, TypeExprKind,
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
    pub(super) rust_types: RustTypeTable,
    pub(super) traits: TraitTable,
    pub(super) generics: HashMap<GenericId, GenericParam>,
    pub(super) schemes: HashMap<BindingId, Scheme>,
    pub(super) component: HashMap<BindingId, Type>,
    pub(super) locals: HashMap<BindingId, Type>,
    pub(super) compile_time_only: HashSet<BindingId>,
    pub(super) diagnostics: Diagnostics,
    pub(super) type_arities: HashMap<BindingId, usize>,
    signatures: HashMap<BindingId, (TypeExpr, Span)>,
    monomorphic: Vec<Type>,
    implied_bounds: HashMap<GenericId, Vec<Bound>>,
    pub(super) field_accessors: HashMap<BindingId, Rc<str>>,
    pub(super) pending_borrows: Vec<PendingBorrow>,
    pub(super) template_borrows: TemplateBorrows,
}

struct Definition<'p> {
    binding: BindingId,
    value: &'p Expr,
    span: Span,
}

type LoweredFields = Vec<(Rc<str>, Type)>;

impl TypeChecker {
    const PARAMETER_NAMES: &str = "abcdefghijklmnopqrstuvwxyz";
    pub const MAIN: &str = "main";

    pub(super) fn new(bindings: BindingArena) -> Self {
        Self {
            bindings,
            unifier: Unifier::default(),
            structs: StructTable::default(),
            rust_types: RustTypeTable::default(),
            traits: TraitTable::default(),
            generics: HashMap::new(),
            schemes: HashMap::new(),
            component: HashMap::new(),
            locals: HashMap::new(),
            compile_time_only: HashSet::new(),
            diagnostics: Diagnostics::default(),
            type_arities: HashMap::new(),
            signatures: HashMap::new(),
            monomorphic: Vec::new(),
            implied_bounds: HashMap::new(),
            field_accessors: HashMap::new(),
            pending_borrows: Vec::new(),
            template_borrows: TemplateBorrows::default(),
        }
    }

    pub(super) fn collect_declarations(&mut self, modules: &[CoreModule]) {
        self.record_type_arities(modules);
        for item in modules.iter().flat_map(|module| &module.items) {
            if let Err(diagnostic) = self.collect_item(item) {
                self.diagnostics.push(diagnostic);
            }
        }
        self.register_derived_traits(modules);
        self.check_trait_claims(modules);
    }

    fn collect_item(&mut self, item: &Item) -> Result<(), Diagnostic> {
        match &item.kind {
            ItemKind::Struct(binding, params, fields) => {
                self.collect_struct(*binding, params, fields)
            }
            ItemKind::RustTrait(binding, path, implementors) => {
                self.collect_trait(*binding, path, implementors)
            }
            ItemKind::RustType(binding, binders, template) => {
                self.collect_rust_type(*binding, binders, template)
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

    fn check_trait_claims(&mut self, modules: &[CoreModule]) {
        for item in modules.iter().flat_map(|module| &module.items) {
            let ItemKind::RustTrait(trait_binding, path, implementors) = &item.kind else {
                continue;
            };
            for implementor in implementors {
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
            if let ItemKind::Struct(binding, params, _) | ItemKind::RustType(binding, params, _) =
                &item.kind
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
            .map(|implementor| implementor.body.clone())
            .collect();
        for rust_trait in others {
            admissible.retain(|candidate| {
                rust_trait
                    .implementors
                    .iter()
                    .any(|implementor| implementor.body == *candidate)
            });
        }
        if traits.iter().any(|rust_trait| rust_trait.is_open()) {
            Admissible::Open(admissible)
        } else {
            Admissible::Closed(admissible)
        }
    }

    pub(super) fn copy_rules(&self) -> super::copy::CopyRules<'_> {
        super::copy::CopyRules::new(&self.traits, &self.generics)
    }

    pub(super) fn display(&self) -> TypeDisplay<'_> {
        TypeDisplay::new(&self.bindings, &self.generics)
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
        let implementor = self.unifier.resolve(&obligation.implementor);
        match implementor {
            Type::Var(var) => self.unifier.add_bound(var, obligation.bound),
            Type::Generic(generic) => {
                self.check_generic_bound(generic, obligation.bound, obligation.span)
            }
            _ => self.select_implementor(obligation, &implementor, pass),
        }
    }

    fn select_implementor(
        &mut self,
        obligation: Obligation,
        implementor: &Type,
        pass: ObligationPass,
    ) {
        match self.traits.select(obligation.bound.binding, implementor) {
            Selection::Implemented(requirements) => {
                for requirement in requirements {
                    let required = Obligation {
                        bound: Bound::plain(requirement.bound),
                        implementor: requirement.implementor,
                        span: obligation.span,
                    };
                    self.discharge(required, pass);
                }
            }
            Selection::Missing if !implementor.has_vars() => {
                self.report_missing_trait(obligation.bound.binding, implementor, obligation.span)
            }
            Selection::Undecided | Selection::Missing => self.leave_unresolved(obligation, pass),
        }
    }

    fn leave_unresolved(&mut self, obligation: Obligation, pass: ObligationPass) {
        match pass {
            ObligationPass::Deferring => self.unifier.defer(obligation),
            ObligationPass::Final => self
                .diagnostics
                .push(TypeProblems::cannot_infer(obligation.span)),
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
        let mut position = start;
        for var in vars {
            if self.generalize_var(var, position, span).is_some() {
                position += 1;
            }
        }
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
        let bounds = self.unifier.bounds(var).to_vec();
        let param = self.new_generic(&name, bounds);
        self.unifier.bind(var, Type::Generic(param.id));
        Some(param)
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
                blocked: Vec::new(),
                written_derive: None,
            },
        );
        borrowed.map_or(Ok(()), |span| {
            Err(TypeProblems::borrowed_struct_field(span))
        })
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

    fn lower_implementor(&mut self, implementor: &TypeExpr) -> Result<Scheme, Diagnostic> {
        let scheme = self.lower_scheme(implementor)?;
        if matches!(scheme.body, Type::Generic(_)) {
            return Err(TypeProblems::bare_implementor(implementor.span()));
        }
        Ok(scheme)
    }

    fn collect_rust_type(
        &mut self,
        binding: BindingId,
        binders: &[TypeBinder],
        template: &std::rc::Rc<ParsedTemplate>,
    ) -> Result<(), Diagnostic> {
        self.reject_bounds(binders)?;
        self.rust_types.add(
            binding,
            RustType {
                template: template.clone(),
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
        let implementors = implementors
            .iter()
            .map(|implementor| self.lower_implementor(implementor))
            .collect::<Result<Vec<Scheme>, Diagnostic>>()?;
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
                    && types.iter().all(|candidate| {
                        self.traits
                            .satisfies(bound.binding, candidate, &|_, _| false)
                    })
            }
            Admissible::Unbounded | Admissible::Open(_) => false,
        }
    }

    fn report_missing_trait(&mut self, bound: BindingId, implementor: &Type, span: Span) {
        let display = self.display();
        let implementors = self
            .traits
            .get(bound)
            .map(|rust_trait| {
                rust_trait
                    .implementors
                    .iter()
                    .map(|known| display.scheme(known))
                    .collect()
            })
            .unwrap_or_default();
        let mut diagnostic = TypeProblems::missing_trait(
            self.bindings.get(bound).name(),
            &display.show(implementor),
            span,
            implementors,
        );
        if let Type::Named(struct_binding, _) = implementor
            && let Some(definition) = self.structs.get(*struct_binding)
        {
            diagnostic = self.with_written_derive(diagnostic, definition, bound);
            if let Some((_, field, field_type)) = definition
                .blocked
                .iter()
                .find(|(trait_binding, _, _)| *trait_binding == bound)
            {
                let struct_name = self.bindings.get(*struct_binding).name().to_string();
                let trait_name = self.bindings.get(bound).name().to_string();
                diagnostic = diagnostic.with_note(TypeProblems::blocked_derive_note(
                    &struct_name,
                    &trait_name,
                    field,
                    &display.show(field_type),
                ));
            }
            if self
                .traits
                .get(bound)
                .is_some_and(|rust_trait| rust_trait.path.as_ref() == "std::fmt::Display")
            {
                diagnostic = diagnostic.with_help(TypeProblems::print_struct_help());
            }
        }
        self.diagnostics.push(diagnostic);
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
        program: crate::expand::Program,
    ) -> Result<super::typed::TypedProgram, Diagnostics> {
        let crate::expand::Program {
            modules,
            entry,
            bindings,
        } = program;
        let mut checker = Self::new(bindings);
        checker.collect_declarations(&modules);
        let mut definitions = checker.check_definitions(&modules, entry);
        let order = Self::definition_order(&modules);
        let mut finalizer = super::finalize::Finalizer::new(&mut checker);
        for binding in &order {
            if let Some(definition) = definitions.get_mut(binding) {
                finalizer.definition(definition);
            }
        }
        checker.discharge_obligations(ObligationPass::Final);
        let typed_modules = modules
            .into_iter()
            .map(|module| Self::typed_module(module, &mut definitions))
            .collect();
        checker.into_program(typed_modules, entry)
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
        module: CoreModule,
        definitions: &mut HashMap<BindingId, TypedDefinition>,
    ) -> super::typed::TypedModule {
        let mut typed = super::typed::TypedModule {
            id: module.id,
            path: module.path,
            definitions: Vec::new(),
            structs: Vec::new(),
            exports: module.exports,
        };
        for item in module.items {
            match item.kind {
                ItemKind::Def(binding, _) => typed.definitions.extend(definitions.remove(&binding)),
                ItemKind::Struct(binding, ..) => typed.structs.push(binding),
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
        let program = super::typed::TypedProgram {
            modules,
            entry,
            bindings: self.bindings,
            structs: self.structs,
            rust_types: self.rust_types,
            traits: self.traits,
            generics: self.generics,
            implied_bounds: self.implied_bounds,
            template_borrows: self.template_borrows,
            warnings: Diagnostics::default(),
        };
        self.diagnostics.into_result(program)
    }
}

#[cfg(test)]
mod tests {
    use super::TypeChecker;

    #[test]
    fn parameters_past_the_alphabet_are_numbered() {
        assert_eq!(TypeChecker::parameter_name(0), "a");
        assert_eq!(TypeChecker::parameter_name(26), "t26");
    }
}
