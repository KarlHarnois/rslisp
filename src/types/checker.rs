use std::collections::{HashMap, HashSet};

use super::display::TypeDisplay;
use super::graph::DependencyGraph;
use super::problems::{MismatchContext, TypeProblems};
use super::structs::{StructDefinition, StructTable};
use super::traits::{RustTrait, TraitTable};
use super::ty::{GenericId, GenericParam, Scheme, Type, TypeVar};
use super::typed::{TypedDefinition, TypedExpr, TypedExprKind, TypedLeaf};
use super::unifier::{LiteralKind, Unifier};
use crate::core::{
    BindingArena, BindingId, CoreModule, Expr, ExprKind, ItemKind, Leaf, ModuleId, PrimitiveType,
    StructDerives, TypeExpr,
};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::source::Span;

pub struct TypeChecker {
    pub(super) bindings: BindingArena,
    pub(super) unifier: Unifier,
    pub(super) structs: StructTable,
    pub(super) traits: TraitTable,
    pub(super) generics: HashMap<GenericId, GenericParam>,
    pub(super) schemes: HashMap<BindingId, Scheme>,
    pub(super) component: HashMap<BindingId, Type>,
    pub(super) locals: HashMap<BindingId, Type>,
    pub(super) compile_time_only: HashSet<BindingId>,
    pub(super) diagnostics: Diagnostics,
    signatures: HashMap<BindingId, (TypeExpr, Span)>,
    monomorphic: Vec<Type>,
    implied_bounds: HashMap<GenericId, Vec<BindingId>>,
}

struct Definition<'p> {
    binding: BindingId,
    value: &'p Expr,
    span: Span,
}

impl TypeChecker {
    const PARAMETER_NAMES: &str = "abcdefghijklmnopqrstuvwxyz";
    pub const MAIN: &str = "main";

    pub(super) fn new(bindings: BindingArena) -> Self {
        Self {
            bindings,
            unifier: Unifier::default(),
            structs: StructTable::default(),
            traits: TraitTable::default(),
            generics: HashMap::new(),
            schemes: HashMap::new(),
            component: HashMap::new(),
            locals: HashMap::new(),
            compile_time_only: HashSet::new(),
            diagnostics: Diagnostics::default(),
            signatures: HashMap::new(),
            monomorphic: Vec::new(),
            implied_bounds: HashMap::new(),
        }
    }

    pub(super) fn collect_declarations(&mut self, modules: &[CoreModule]) {
        for item in modules.iter().flat_map(|module| &module.items) {
            let collected = match &item.kind {
                ItemKind::Struct(binding, fields) => self.collect_struct(*binding, fields),
                ItemKind::RustTrait(binding, path, implementors) => {
                    self.collect_trait(*binding, path, implementors)
                }
                ItemKind::Signature(binding, signature) => {
                    self.signatures
                        .insert(*binding, (signature.clone(), item.span));
                    Ok(())
                }
                ItemKind::Def(..) => Ok(()),
            };
            if let Err(diagnostic) = collected {
                self.diagnostics.push(diagnostic);
            }
        }
        self.register_derived_traits(modules);
    }

    fn register_derived_traits(&mut self, modules: &[CoreModule]) {
        for item in modules.iter().flat_map(|module| &module.items) {
            if let ItemKind::Struct(binding, _) = item.kind {
                self.traits
                    .add_implementor_to_paths(&StructDerives::TRAITS, Type::Struct(binding));
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

    pub(super) fn admissible_types(&self, generic: GenericId) -> Option<Vec<Type>> {
        let bounds = &self.generics.get(&generic)?.bounds;
        let (first, others) = bounds.split_first()?;
        let mut admissible = self.traits.get(*first)?.implementors.clone();
        for bound in others {
            let implementors = self
                .traits
                .get(*bound)
                .map(|rust_trait| rust_trait.implementors.as_slice())
                .unwrap_or_default();
            admissible.retain(|candidate| implementors.contains(candidate));
        }
        Some(admissible)
    }

    pub(super) fn copy_rules(&self) -> super::copy::CopyRules<'_> {
        super::copy::CopyRules::new(&self.traits, &self.generics)
    }

    pub(super) fn display(&self) -> TypeDisplay<'_> {
        TypeDisplay::new(&self.bindings, &self.generics)
    }

    pub(super) fn new_generic(&mut self, name: &str, bounds: Vec<BindingId>) -> GenericParam {
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

    pub(super) fn discharge_obligations(&mut self, final_pass: bool) {
        for obligation in self.unifier.take_obligations() {
            let implementor = self.unifier.resolve(&obligation.implementor);
            match implementor {
                Type::Var(var) => self.unifier.add_bound(var, obligation.bound),
                Type::Generic(generic) => {
                    self.check_generic_bound(generic, obligation.bound, obligation.span)
                }
                _ if implementor.has_vars() && !final_pass => self.unifier.defer(obligation),
                _ if implementor.has_vars() => self
                    .diagnostics
                    .push(TypeProblems::cannot_infer(obligation.span)),
                _ => self.check_implemented(obligation.bound, &implementor, obligation.span),
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
        self.discharge_obligations(false);
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

    fn assign_rust_scheme(&mut self, member: &Definition) -> bool {
        let ExprKind::Leaf(Leaf::Rust(template)) = member.value.kind() else {
            return false;
        };
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
            let fresh = self.unifier.fresh();
            self.component.insert(member.binding, fresh);
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
        let inferred = self.component[&binding].clone();
        if let Some(signature) = signature {
            return Scheme {
                params: signature.params.clone(),
                body: self.unifier.resolve(&inferred),
            };
        }
        if matches!(value.kind, TypedExprKind::Fn(_)) {
            return self.generalize(&inferred, value.span);
        }
        self.monomorphic.push(inferred.clone());
        Scheme::monomorphic(self.unifier.resolve(&inferred))
    }

    fn generalize(&mut self, inferred: &Type, span: Span) -> Scheme {
        let excluded = self.monomorphic_vars();
        let mut params = Vec::new();
        for var in self.free_vars(inferred) {
            if excluded.contains(&var) {
                continue;
            }
            if let Some(param) = self.generalize_var(var, params.len(), span) {
                params.push(param);
            }
        }
        self.discharge_obligations(false);
        Scheme {
            params,
            body: self.unifier.resolve(inferred),
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

    fn parameter_name(position: usize) -> String {
        let letters: Vec<char> = Self::PARAMETER_NAMES.chars().collect();
        match letters.get(position) {
            Some(letter) => letter.to_string(),
            None => format!("t{position}"),
        }
    }

    fn collect_struct(
        &mut self,
        binding: BindingId,
        fields: &[(crate::core::FieldLabel, TypeExpr)],
    ) -> Result<(), Diagnostic> {
        let mut lowered = Vec::new();
        for (label, field_type) in fields {
            lowered.push((
                label.name.clone(),
                self.lower_type(field_type, &HashMap::new())?,
            ));
        }
        self.structs
            .add(binding, StructDefinition { fields: lowered });
        Ok(())
    }

    fn collect_trait(
        &mut self,
        binding: BindingId,
        path: &std::rc::Rc<str>,
        implementors: &[TypeExpr],
    ) -> Result<(), Diagnostic> {
        let implementors = implementors
            .iter()
            .map(|implementor| self.lower_type(implementor, &HashMap::new()))
            .collect::<Result<Vec<Type>, Diagnostic>>()?;
        self.traits.add(
            binding,
            RustTrait {
                path: path.clone(),
                implementors,
            },
        );
        Ok(())
    }

    fn check_generic_bound(&mut self, generic: GenericId, bound: BindingId, span: Span) {
        if self.unifier.generic_bounds(generic).contains(&bound) {
            return;
        }
        if self.implies(generic, bound) {
            let implied = self.implied_bounds.entry(generic).or_default();
            if !implied.contains(&bound) {
                implied.push(bound);
            }
            return;
        }
        let generic_name = self.display().show(&Type::Generic(generic));
        let diagnostic =
            TypeProblems::missing_bound(self.bindings.get(bound).name(), &generic_name, span);
        self.diagnostics.push(diagnostic);
    }

    fn implies(&self, generic: GenericId, bound: BindingId) -> bool {
        self.admissible_types(generic).is_some_and(|types| {
            !types.is_empty()
                && types
                    .iter()
                    .all(|candidate| self.traits.is_implemented(bound, candidate))
        })
    }

    fn check_implemented(&mut self, bound: BindingId, implementor: &Type, span: Span) {
        if self.traits.is_implemented(bound, implementor) {
            return;
        }
        let display = self.display();
        let implementors = self
            .traits
            .get(bound)
            .map(|rust_trait| {
                rust_trait
                    .implementors
                    .iter()
                    .map(|known| display.show(known))
                    .collect()
            })
            .unwrap_or_default();
        let diagnostic = TypeProblems::missing_trait(
            self.bindings.get(bound).name(),
            &display.show(implementor),
            span,
            implementors,
        );
        self.diagnostics.push(diagnostic);
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
        checker.discharge_obligations(true);
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
                ItemKind::Struct(binding, _) => typed.structs.push(binding),
                ItemKind::Signature(..) | ItemKind::RustTrait(..) => {}
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
            traits: self.traits,
            generics: self.generics,
            implied_bounds: self.implied_bounds,
        };
        self.diagnostics.into_result(program)
    }
}
