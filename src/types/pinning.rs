use std::collections::{BTreeMap, HashMap, HashSet};

use super::display::TypeDisplay;
use super::graph::DefinitionGraph;
use super::ty::{GenericId, Type};
use super::typed::{TypedDefinition, TypedExpr, TypedExprKind, TypedLeaf, TypedProgram};
use crate::core::{BindingId, ItemKind, PrimitiveType};
use crate::expand::Program;

struct UseTypes {
    types: Vec<Option<Type>>,
    resolved: bool,
}

struct UseSite {
    enclosing: BindingId,
    actual: Type,
}

struct FunctionUses {
    params: Vec<GenericId>,
    literal: Vec<(GenericId, PrimitiveType)>,
    body: Type,
    sites: Vec<UseSite>,
    uses: Vec<UseTypes>,
    written: bool,
}

struct Target {
    params: Vec<GenericId>,
    literal: Vec<(GenericId, PrimitiveType)>,
    body: Type,
}

pub struct InstantiationReport {
    entries: BTreeMap<BindingId, FunctionUses>,
    substitutions: HashMap<BindingId, HashMap<GenericId, Type>>,
}

impl InstantiationReport {
    pub fn of(program: &Program, graph: &DefinitionGraph, typed: &TypedProgram) -> Self {
        let written = Self::written_signatures(program);
        let groups = Self::recursive_groups(graph);
        let targets = Self::targets(typed);
        let mut entries = Self::entries(&targets, &written);
        Self::collect_uses(typed, &targets, &groups, &mut entries);
        let substitutions = Self::group_substitutions(graph, &mut entries);
        Self {
            entries,
            substitutions,
        }
    }

    pub fn one_type(&self, binding: BindingId) -> Option<HashMap<GenericId, Type>> {
        self.substitutions.get(&binding).cloned()
    }

    pub fn uses_of(&self, binding: BindingId, generic: GenericId) -> Vec<Option<Type>> {
        let Some(entry) = self.entries.get(&binding) else {
            return Vec::new();
        };
        let Some(index) = entry.params.iter().position(|param| *param == generic) else {
            return Vec::new();
        };
        entry
            .uses
            .iter()
            .map(|used| used.types[index].clone())
            .collect()
    }

    pub fn is_used(&self, binding: BindingId) -> bool {
        self.entries
            .get(&binding)
            .is_some_and(|entry| !entry.sites.is_empty())
    }

    fn written_signatures(program: &Program) -> HashSet<BindingId> {
        program
            .modules
            .iter()
            .flat_map(|module| &module.items)
            .filter_map(|item| match &item.kind {
                ItemKind::Signature(binding, _) => Some(*binding),
                _ => None,
            })
            .collect()
    }

    fn recursive_groups(graph: &DefinitionGraph) -> HashMap<BindingId, usize> {
        let mut groups = HashMap::new();
        for (index, component) in graph.components().iter().enumerate() {
            for binding in component {
                groups.insert(*binding, index);
            }
        }
        groups
    }

    fn targets(typed: &TypedProgram) -> HashMap<BindingId, Target> {
        typed
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .filter(|definition| {
                matches!(definition.value.kind, TypedExprKind::Fn(_))
                    && !definition.scheme.params.is_empty()
            })
            .map(|definition| {
                let target = Target {
                    params: definition
                        .scheme
                        .params
                        .iter()
                        .map(|param| param.id)
                        .collect(),
                    literal: definition
                        .scheme
                        .params
                        .iter()
                        .filter_map(|param| {
                            typed
                                .literal_generics
                                .get(&param.id)
                                .map(|literal| (param.id, literal.default_type()))
                        })
                        .collect(),
                    body: definition.scheme.body.clone(),
                };
                (definition.binding, target)
            })
            .collect()
    }

    fn entries(
        targets: &HashMap<BindingId, Target>,
        written: &HashSet<BindingId>,
    ) -> BTreeMap<BindingId, FunctionUses> {
        targets
            .iter()
            .map(|(binding, target)| {
                let entry = FunctionUses {
                    params: target.params.clone(),
                    literal: target.literal.clone(),
                    body: target.body.clone(),
                    sites: Vec::new(),
                    uses: Vec::new(),
                    written: written.contains(binding),
                };
                (*binding, entry)
            })
            .collect()
    }

    fn collect_uses(
        typed: &TypedProgram,
        targets: &HashMap<BindingId, Target>,
        groups: &HashMap<BindingId, usize>,
        entries: &mut BTreeMap<BindingId, FunctionUses>,
    ) {
        for module in &typed.modules {
            for definition in &module.definitions {
                Self::collect_expr(
                    &definition.value,
                    definition.binding,
                    targets,
                    groups,
                    entries,
                );
            }
        }
    }

    fn collect_expr(
        expr: &TypedExpr,
        enclosing: BindingId,
        targets: &HashMap<BindingId, Target>,
        groups: &HashMap<BindingId, usize>,
        entries: &mut BTreeMap<BindingId, FunctionUses>,
    ) {
        if let TypedExprKind::Call(callee, arguments) = &expr.kind {
            Self::collect_callee(callee, arguments, enclosing, targets, groups, entries);
            return;
        }
        if let TypedExprKind::Leaf(TypedLeaf::Global(binding)) = &expr.kind {
            Self::record(*binding, &expr.ty, enclosing, targets, groups, entries);
        }
        for child in expr.children() {
            Self::collect_expr(child, enclosing, targets, groups, entries);
        }
    }

    fn collect_callee(
        callee: &TypedExpr,
        arguments: &[super::typed::TypedArgument],
        enclosing: BindingId,
        targets: &HashMap<BindingId, Target>,
        groups: &HashMap<BindingId, usize>,
        entries: &mut BTreeMap<BindingId, FunctionUses>,
    ) {
        match &callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Global(binding)) => {
                Self::record(*binding, &callee.ty, enclosing, targets, groups, entries);
            }
            _ => Self::collect_expr(callee, enclosing, targets, groups, entries),
        }
        for argument in arguments {
            Self::collect_expr(&argument.value, enclosing, targets, groups, entries);
        }
    }

    fn record(
        target_binding: BindingId,
        actual: &Type,
        enclosing: BindingId,
        targets: &HashMap<BindingId, Target>,
        groups: &HashMap<BindingId, usize>,
        entries: &mut BTreeMap<BindingId, FunctionUses>,
    ) {
        if groups.get(&enclosing) == groups.get(&target_binding) {
            return;
        }
        if !targets.contains_key(&target_binding) {
            return;
        }
        let entry = entries
            .get_mut(&target_binding)
            .expect("every target has an entry");
        entry.sites.push(UseSite {
            enclosing,
            actual: actual.clone(),
        });
    }

    fn use_types(body: &Type, params: &[GenericId], actual: &Type) -> UseTypes {
        let mut bound = HashMap::new();
        let matched = Self::bind(body, actual, &mut bound);
        let types: Vec<Option<Type>> = params
            .iter()
            .map(|param| bound.get(param).cloned())
            .collect();
        let resolved = matched
            && types
                .iter()
                .all(|ty| ty.as_ref().is_some_and(Type::is_concrete));
        UseTypes { types, resolved }
    }

    fn bind(scheme: &Type, actual: &Type, bound: &mut HashMap<GenericId, Type>) -> bool {
        match scheme {
            Type::Generic(generic) => {
                bound.insert(*generic, actual.clone());
                true
            }
            Type::Named(..) => Self::bind_named(scheme, actual, bound),
            Type::Function(..) => Self::bind_function(scheme, actual, bound),
            Type::Ref(..) | Type::Mut(..) => Self::bind_reference(scheme, actual, bound),
            Type::Primitive(primitive) => {
                matches!(actual, Type::Primitive(other) if primitive == other)
            }
            Type::Var(_) | Type::TraitObject(_) => false,
        }
    }

    fn bind_named(scheme: &Type, actual: &Type, bound: &mut HashMap<GenericId, Type>) -> bool {
        let (Type::Named(left, left_arguments), Type::Named(right, right_arguments)) =
            (scheme, actual)
        else {
            return false;
        };
        left == right
            && left_arguments.len() == right_arguments.len()
            && Self::bind_all(left_arguments, right_arguments, bound)
    }

    fn bind_function(scheme: &Type, actual: &Type, bound: &mut HashMap<GenericId, Type>) -> bool {
        let (
            Type::Function(_, left_parameters, left_result),
            Type::Function(_, right_parameters, right_result),
        ) = (scheme, actual)
        else {
            return false;
        };
        left_parameters.len() == right_parameters.len()
            && Self::bind_all(left_parameters, right_parameters, bound)
            && Self::bind(left_result, right_result, bound)
    }

    fn bind_reference(scheme: &Type, actual: &Type, bound: &mut HashMap<GenericId, Type>) -> bool {
        match (scheme, actual) {
            (Type::Ref(_, left), Type::Ref(_, right))
            | (Type::Mut(_, left), Type::Mut(_, right)) => Self::bind(left, right, bound),
            _ => false,
        }
    }

    fn bind_all(scheme: &[Type], actual: &[Type], bound: &mut HashMap<GenericId, Type>) -> bool {
        scheme
            .iter()
            .zip(actual)
            .all(|(scheme, actual)| Self::bind(scheme, actual, bound))
    }

    fn group_substitutions(
        graph: &DefinitionGraph,
        entries: &mut BTreeMap<BindingId, FunctionUses>,
    ) -> HashMap<BindingId, HashMap<GenericId, Type>> {
        let mut substitutions = HashMap::new();
        for component in graph.components().iter().rev() {
            let members: Vec<BindingId> = component
                .iter()
                .filter(|binding| entries.contains_key(binding))
                .copied()
                .collect();
            Self::resolve_group_uses(&members, entries, &substitutions);
            if let Some(substitution) = Self::decide_group(&members, entries)
                && !Self::only_literal_defaults(&members, entries, &substitution)
            {
                for member in &members {
                    substitutions.insert(*member, substitution.clone());
                }
            }
        }
        substitutions
    }

    fn resolve_group_uses(
        members: &[BindingId],
        entries: &mut BTreeMap<BindingId, FunctionUses>,
        callers: &HashMap<BindingId, HashMap<GenericId, Type>>,
    ) {
        for member in members {
            let entry = entries.get_mut(member).expect("every member has an entry");
            entry.uses = entry
                .sites
                .iter()
                .map(|site| Self::resolved_use(entry, site, callers))
                .collect();
        }
    }

    fn resolved_use(
        entry: &FunctionUses,
        site: &UseSite,
        callers: &HashMap<BindingId, HashMap<GenericId, Type>>,
    ) -> UseTypes {
        match callers.get(&site.enclosing) {
            Some(caller) => {
                let actual = site.actual.substitute(caller);
                Self::use_types(&entry.body, &entry.params, &actual)
            }
            None => Self::use_types(&entry.body, &entry.params, &site.actual),
        }
    }

    fn only_literal_defaults(
        members: &[BindingId],
        entries: &BTreeMap<BindingId, FunctionUses>,
        substitution: &HashMap<GenericId, Type>,
    ) -> bool {
        substitution.iter().all(|(generic, ty)| {
            members.iter().any(|member| {
                entries[member]
                    .literal
                    .iter()
                    .any(|(id, primitive)| id == generic && *ty == Type::Primitive(*primitive))
            })
        })
    }

    fn decide_group(
        members: &[BindingId],
        entries: &BTreeMap<BindingId, FunctionUses>,
    ) -> Option<HashMap<GenericId, Type>> {
        if members.iter().any(|member| entries[member].written) {
            return None;
        }
        let combined = Self::combined_uses(members, entries)?;
        members
            .iter()
            .all(|member| Self::covers_params(&entries[member], &combined))
            .then_some(combined)
    }

    fn combined_uses(
        members: &[BindingId],
        entries: &BTreeMap<BindingId, FunctionUses>,
    ) -> Option<HashMap<GenericId, Type>> {
        let mut combined: HashMap<GenericId, Type> = HashMap::new();
        let mut uses = 0;
        for member in members {
            for used in &entries[member].uses {
                if !used.resolved {
                    return None;
                }
                uses += 1;
                Self::merge_use(&mut combined, &entries[member].params, used)?;
            }
        }
        (uses > 0).then_some(combined)
    }

    fn merge_use(
        combined: &mut HashMap<GenericId, Type>,
        params: &[GenericId],
        used: &UseTypes,
    ) -> Option<()> {
        for (param, ty) in params.iter().zip(&used.types) {
            let ty = ty.clone().expect("a resolved use names every parameter");
            match combined.entry(*param) {
                std::collections::hash_map::Entry::Occupied(existing) => {
                    if *existing.get() != ty {
                        return None;
                    }
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(ty);
                }
            }
        }
        Some(())
    }

    fn covers_params(entry: &FunctionUses, combined: &HashMap<GenericId, Type>) -> bool {
        entry
            .params
            .iter()
            .all(|param| combined.contains_key(param))
    }
}

#[derive(Clone, Default)]
pub struct LiteralDefaults {
    entries: HashMap<(BindingId, usize), LiteralDefault>,
}

#[derive(Clone)]
pub struct LiteralDefault {
    pub parameter: String,
    pub default: String,
    pub calls: Vec<String>,
    pub signature: String,
}

impl LiteralDefaults {
    pub fn of(report: &InstantiationReport, typed: &TypedProgram) -> Self {
        let display = TypeDisplay::new(&typed.bindings, &typed.generics)
            .with_projections(typed.traits.projections());
        let mut defaults = Self::default();
        for definition in typed.modules.iter().flat_map(|module| &module.definitions) {
            defaults.collect(report, typed, definition, &display);
        }
        defaults
    }

    pub fn get(&self, binding: BindingId, position: usize) -> Option<&LiteralDefault> {
        self.entries.get(&(binding, position))
    }

    fn collect(
        &mut self,
        report: &InstantiationReport,
        typed: &TypedProgram,
        definition: &TypedDefinition,
        display: &TypeDisplay<'_>,
    ) {
        let TypedExprKind::Fn(lambda) = &definition.value.kind else {
            return;
        };
        let Type::Function(_, parameters, result) = &definition.scheme.body else {
            return;
        };
        let Some(entry) = report.entries.get(&definition.binding) else {
            return;
        };
        if entry.written {
            return;
        }
        for (position, parameter) in parameters.iter().enumerate() {
            let Type::Generic(generic) = parameter else {
                continue;
            };
            let Some(literal) = typed.literal_generics.get(generic) else {
                continue;
            };
            let default = literal.default_type();
            let Some(generic_index) = Self::generic_index(definition, *generic) else {
                continue;
            };
            let calls = Self::call_types(entry, generic_index, display);
            if calls.len() < 2 {
                continue;
            }
            let Some(binder) = lambda.params.get(position) else {
                continue;
            };
            self.entries.insert(
                (definition.binding, position),
                LiteralDefault {
                    parameter: typed.bindings.get(binder.binding).name().to_string(),
                    default: default.name().to_string(),
                    calls,
                    signature: Self::signature(
                        definition, entry, typed, parameters, result, display,
                    ),
                },
            );
        }
    }

    fn generic_index(definition: &TypedDefinition, generic: GenericId) -> Option<usize> {
        definition
            .scheme
            .params
            .iter()
            .position(|param| param.id == generic)
    }

    fn call_types(
        entry: &FunctionUses,
        generic_index: usize,
        display: &TypeDisplay<'_>,
    ) -> Vec<String> {
        let mut calls = Vec::new();
        for used in &entry.uses {
            let Some(Some(call)) = used.types.get(generic_index) else {
                continue;
            };
            let shown = display.show(call);
            if !calls.contains(&shown) {
                calls.push(shown);
            }
        }
        calls
    }

    fn signature(
        definition: &TypedDefinition,
        entry: &FunctionUses,
        typed: &TypedProgram,
        parameters: &[Type],
        result: &Type,
        display: &TypeDisplay<'_>,
    ) -> String {
        let parameters: Vec<String> = parameters
            .iter()
            .map(|parameter| Self::shown_type(definition, entry, typed, parameter, display))
            .collect();
        format!(
            "(fn ({}) {})",
            parameters.join(" "),
            Self::shown_type(definition, entry, typed, result, display)
        )
    }

    fn shown_type(
        definition: &TypedDefinition,
        entry: &FunctionUses,
        typed: &TypedProgram,
        shown: &Type,
        display: &TypeDisplay<'_>,
    ) -> String {
        let Type::Generic(generic) = shown else {
            return display.show(shown);
        };
        let Some(literal) = typed.literal_generics.get(generic) else {
            return display.show(shown);
        };
        let Some(generic_index) = Self::generic_index(definition, *generic) else {
            return display.show(shown);
        };
        Self::call_types(entry, generic_index, display)
            .first()
            .cloned()
            .unwrap_or_else(|| literal.default_type().name().to_string())
    }
}

pub struct Pinning {
    pins: Vec<(BindingId, Type)>,
}

impl Pinning {
    pub fn of(report: &InstantiationReport) -> Self {
        let pins = report
            .entries
            .iter()
            .filter_map(|(binding, entry)| {
                let substitution = report.one_type(*binding)?;
                Some((*binding, entry.body.substitute(&substitution)))
            })
            .collect();
        Self { pins }
    }

    pub fn pins(&self) -> &[(BindingId, Type)] {
        &self.pins
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use std::collections::HashSet;

    use super::{InstantiationReport, Pinning};
    use crate::core::{
        BindingArena, BindingId, BindingInfo, BindingKind, ClosureKind, ModuleId, PrimitiveType,
    };
    use crate::types::{DefinitionGraph, GenericId, Type, TypeVar, TypedSource};

    struct BindFixture {
        bound: HashMap<GenericId, Type>,
        named: BindingId,
    }

    impl BindFixture {
        fn new() -> Self {
            let mut bindings = BindingArena::default();
            let named = bindings.add(BindingInfo::new(
                "item".into(),
                BindingKind::Global(ModuleId::new(0)),
                None,
            ));
            Self {
                bound: HashMap::new(),
                named,
            }
        }

        fn bind(&mut self, scheme: &Type, actual: &Type) -> bool {
            InstantiationReport::bind(scheme, actual, &mut self.bound)
        }

        fn primitive(&self) -> Type {
            Type::Primitive(PrimitiveType::I64)
        }
    }

    #[test]
    fn a_type_variable_scheme_does_not_bind() {
        let mut fixture = BindFixture::new();
        let scheme = Type::Var(TypeVar::new(0));
        assert!(!fixture.bind(&scheme, &fixture.primitive()));
    }

    #[test]
    fn a_named_scheme_does_not_bind_a_primitive() {
        let mut fixture = BindFixture::new();
        let scheme = Type::Named(fixture.named, Vec::new());
        assert!(!fixture.bind(&scheme, &fixture.primitive()));
    }

    #[test]
    fn a_function_scheme_does_not_bind_a_primitive() {
        let mut fixture = BindFixture::new();
        let scheme = Type::Function(ClosureKind::Fn, Vec::new(), Box::new(Type::unit()));
        assert!(!fixture.bind(&scheme, &fixture.primitive()));
    }

    #[test]
    fn a_reference_scheme_does_not_bind_a_primitive() {
        let mut fixture = BindFixture::new();
        let scheme = Type::shared(Type::Primitive(PrimitiveType::I64));
        assert!(!fixture.bind(&scheme, &fixture.primitive()));
    }

    struct ChainFixture {
        source: TypedSource,
    }

    impl ChainFixture {
        const WRAPPER_CHAIN: &str = "
            (defn inner (x) x)
            (defn middle (x) (inner x))
            (defn outer (x) (middle x))
            (defn main () (println (outer (the u16 1))))";

        fn new() -> Self {
            Self {
                source: TypedSource::expand(Self::WRAPPER_CHAIN),
            }
        }

        fn pinned_after_one_run(&self) -> HashSet<String> {
            let program = self.source.program();
            let graph = DefinitionGraph::of(program);
            let typed = self.source.check(&graph);
            let report = InstantiationReport::of(program, &graph, &typed);
            Pinning::of(&report)
                .pins()
                .iter()
                .map(|(binding, _)| program.bindings.get(*binding).name().to_string())
                .collect()
        }
    }

    #[test]
    fn one_run_pins_every_wrapper_that_a_pinned_caller_reaches() {
        let pinned = ChainFixture::new().pinned_after_one_run();
        let expected = HashSet::from(["inner", "middle", "outer"].map(String::from));
        assert_eq!(pinned, expected);
    }
}
