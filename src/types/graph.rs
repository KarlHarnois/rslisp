use rustc_hash::{FxHashMap, FxHashSet};

use crate::core::{BindingArena, BindingId, BindingKind, Expr, ExprKind, ItemKind, Leaf, ModuleId};
use crate::expand::Program;
use crate::modules::ModuleSources;
use crate::source::Span;

pub struct Definition<'p> {
    pub binding: BindingId,
    pub value: &'p Expr,
    pub span: Span,
}

pub struct DefinitionGraph<'p> {
    definitions: Vec<Definition<'p>>,
    dependencies: DependencyGraph,
    components: Vec<Vec<BindingId>>,
}

impl<'p> DefinitionGraph<'p> {
    pub fn of(program: &'p Program) -> Self {
        Self::deferring(program, Self::prelude_macro_helpers(program))
    }

    pub fn with_every_definition(program: &'p Program) -> Self {
        Self::deferring(program, FxHashSet::default())
    }

    fn deferring(program: &'p Program, macro_helpers: FxHashSet<BindingId>) -> Self {
        let walked = ReferenceWalk::new(&program.bindings, &macro_helpers)
            .run(Self::definitions_in(program));
        let mut definitions = Vec::with_capacity(Self::item_count(program));
        definitions.extend(
            Self::definitions_in(program)
                .filter(|definition| walked.contains_key(&definition.binding)),
        );
        let dependencies = DependencyGraph::build(walked, macro_helpers);
        let order: Vec<BindingId> = definitions
            .iter()
            .map(|definition| definition.binding)
            .collect();
        let components = dependencies.components(&order);
        Self {
            definitions,
            dependencies,
            components,
        }
    }

    fn prelude_macro_helpers(program: &Program) -> FxHashSet<BindingId> {
        let prelude: FxHashSet<ModuleId> = program
            .modules
            .iter()
            .filter(|module| ModuleSources::is_prelude(&module.path))
            .map(|module| module.id)
            .collect();
        program
            .macro_helpers
            .iter()
            .filter(|binding| {
                matches!(program.bindings.get(**binding).kind(),
                    BindingKind::Global(module) if prelude.contains(&module))
            })
            .copied()
            .collect()
    }

    fn item_count(program: &Program) -> usize {
        program
            .modules
            .iter()
            .map(|module| module.items.len())
            .sum()
    }

    fn definitions_in(program: &'p Program) -> impl Iterator<Item = Definition<'p>> {
        program
            .modules
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
    }

    pub fn definitions(&self) -> &[Definition<'p>] {
        &self.definitions
    }

    pub fn dependencies(&self) -> &DependencyGraph {
        &self.dependencies
    }

    pub fn order(&self) -> impl Iterator<Item = BindingId> + '_ {
        self.definitions.iter().map(|definition| definition.binding)
    }

    pub fn components(&self) -> &[Vec<BindingId>] {
        &self.components
    }
}

struct ReferenceWalk<'a, 'p> {
    bindings: &'a BindingArena,
    deferred: &'a FxHashSet<BindingId>,
    waiting: FxHashMap<BindingId, &'p Expr>,
    reached: Vec<BindingId>,
    walked: FxHashMap<BindingId, References>,
}

impl<'a, 'p> ReferenceWalk<'a, 'p> {
    fn new(bindings: &'a BindingArena, deferred: &'a FxHashSet<BindingId>) -> Self {
        Self {
            bindings,
            deferred,
            waiting: FxHashMap::default(),
            reached: Vec::new(),
            walked: FxHashMap::default(),
        }
    }

    fn run(
        mut self,
        definitions: impl Iterator<Item = Definition<'p>>,
    ) -> FxHashMap<BindingId, References> {
        for root in self.set_aside_deferred(definitions) {
            self.walk(root.binding, root.value);
        }
        while let Some(binding) = self.reached.pop() {
            if let Some(value) = self.waiting.remove(&binding) {
                self.walk(binding, value);
            }
        }
        self.walked
    }

    fn set_aside_deferred(
        &mut self,
        definitions: impl Iterator<Item = Definition<'p>>,
    ) -> Vec<Definition<'p>> {
        let (deferred, roots): (Vec<_>, Vec<_>) =
            definitions.partition(|definition| self.deferred.contains(&definition.binding));
        self.waiting = deferred
            .into_iter()
            .map(|definition| (definition.binding, definition.value))
            .collect();
        roots
    }

    fn walk(&mut self, binding: BindingId, value: &Expr) {
        let mut references = References::default();
        references.collect(value, self.bindings);
        self.reached.extend(
            references
                .globals
                .iter()
                .filter(|global| self.waiting.contains_key(global)),
        );
        self.walked.insert(binding, references);
    }
}

pub struct DependencyGraph {
    edges: FxHashMap<BindingId, Vec<BindingId>>,
    compile_time_only: FxHashSet<BindingId>,
    macro_helpers: FxHashSet<BindingId>,
}

impl DependencyGraph {
    fn build(
        walked: FxHashMap<BindingId, References>,
        macro_helpers: FxHashSet<BindingId>,
    ) -> Self {
        let mut edges = FxHashMap::with_capacity_and_hasher(walked.len(), Default::default());
        let mut direct_compile_time = FxHashSet::default();
        for (binding, references) in &walked {
            if references.uses_compile_time_code {
                direct_compile_time.insert(*binding);
            }
            edges.insert(
                *binding,
                references
                    .globals
                    .iter()
                    .filter(|global| walked.contains_key(global))
                    .copied()
                    .collect(),
            );
        }
        let mut graph = Self {
            macro_helpers: macro_helpers
                .into_iter()
                .filter(|helper| walked.contains_key(helper))
                .collect(),
            edges,
            compile_time_only: direct_compile_time,
        };
        graph.spread_compile_time();
        graph
    }

    pub fn is_compile_time_only(&self, binding: BindingId) -> bool {
        self.compile_time_only.contains(&binding)
    }

    pub fn unchecked(&self, mains: &[BindingId]) -> FxHashSet<BindingId> {
        let runtime = self.reachable_from(mains);
        let checked_helpers = self.reachable_from(&self.runtime_roots(mains));
        self.edges
            .keys()
            .filter(|binding| {
                (self.is_compile_time_only(**binding) && !runtime.contains(binding))
                    || (self.macro_helpers.contains(binding) && !checked_helpers.contains(binding))
            })
            .copied()
            .collect()
    }

    fn runtime_roots(&self, mains: &[BindingId]) -> Vec<BindingId> {
        self.edges
            .keys()
            .filter(|binding| {
                !self.is_compile_time_only(**binding) && !self.macro_helpers.contains(binding)
            })
            .chain(mains)
            .copied()
            .collect()
    }

    pub fn directly_depends_on_any(
        &self,
        roots: &[BindingId],
        bindings: &FxHashSet<BindingId>,
    ) -> bool {
        roots
            .iter()
            .filter_map(|root| self.edges.get(root))
            .flatten()
            .any(|dependency| bindings.contains(dependency))
    }

    pub fn reachable_from(&self, roots: &[BindingId]) -> FxHashSet<BindingId> {
        let mut reached: FxHashSet<BindingId> = FxHashSet::default();
        let mut pending: Vec<BindingId> = roots.to_vec();
        while let Some(binding) = pending.pop() {
            if reached.insert(binding) {
                pending.extend(self.edges.get(&binding).into_iter().flatten().copied());
            }
        }
        reached
    }

    pub fn components(&self, order: &[BindingId]) -> Vec<Vec<BindingId>> {
        StronglyConnected::new(&self.edges).run(order)
    }

    fn spread_compile_time(&mut self) {
        let mut changed = true;
        while changed {
            changed = false;
            for (binding, dependencies) in &self.edges {
                let depends_on_compile_time = dependencies
                    .iter()
                    .any(|dependency| self.compile_time_only.contains(dependency));
                if depends_on_compile_time && !self.compile_time_only.contains(binding) {
                    self.compile_time_only.insert(*binding);
                    changed = true;
                }
            }
        }
    }
}

#[derive(Default)]
struct References {
    globals: Vec<BindingId>,
    uses_compile_time_code: bool,
}

impl References {
    fn collect(&mut self, expr: &Expr, bindings: &BindingArena) {
        match expr.kind() {
            ExprKind::Leaf(Leaf::Var(binding)) => self.note_reference(*binding, bindings),
            ExprKind::Leaf(Leaf::Quote(_)) => self.uses_compile_time_code = true,
            _ => {}
        }
        for child in expr.children() {
            self.collect(child, bindings);
        }
    }

    fn note_reference(&mut self, binding: BindingId, bindings: &BindingArena) {
        match bindings.get(binding).kind() {
            BindingKind::Global(_) => self.globals.push(binding),
            BindingKind::CodeBuiltin(_) => self.uses_compile_time_code = true,
            _ => {}
        }
    }
}

struct StronglyConnected<'a> {
    edges: &'a FxHashMap<BindingId, Vec<BindingId>>,
    index: FxHashMap<BindingId, usize>,
    low_link: FxHashMap<BindingId, usize>,
    stack: Vec<BindingId>,
    on_stack: FxHashSet<BindingId>,
    components: Vec<Vec<BindingId>>,
}

impl<'a> StronglyConnected<'a> {
    fn new(edges: &'a FxHashMap<BindingId, Vec<BindingId>>) -> Self {
        Self {
            edges,
            index: FxHashMap::default(),
            low_link: FxHashMap::default(),
            stack: Vec::new(),
            on_stack: FxHashSet::default(),
            components: Vec::new(),
        }
    }

    fn run(mut self, order: &[BindingId]) -> Vec<Vec<BindingId>> {
        for binding in order {
            if !self.index.contains_key(binding) {
                self.visit(*binding);
            }
        }
        self.components
    }

    fn visit(&mut self, binding: BindingId) {
        let position = self.index.len();
        self.index.insert(binding, position);
        self.low_link.insert(binding, position);
        self.stack.push(binding);
        self.on_stack.insert(binding);
        for dependency in self.edges.get(&binding).cloned().unwrap_or_default() {
            self.visit_dependency(binding, dependency);
        }
        if self.low_link[&binding] == self.index[&binding] {
            self.pop_component(binding);
        }
    }

    fn visit_dependency(&mut self, binding: BindingId, dependency: BindingId) {
        if !self.index.contains_key(&dependency) {
            self.visit(dependency);
            let lowest = self.low_link[&binding].min(self.low_link[&dependency]);
            self.low_link.insert(binding, lowest);
        } else if self.on_stack.contains(&dependency) {
            let lowest = self.low_link[&binding].min(self.index[&dependency]);
            self.low_link.insert(binding, lowest);
        }
    }

    fn pop_component(&mut self, root: BindingId) {
        let mut component = Vec::new();
        while let Some(member) = self.stack.pop() {
            self.on_stack.remove(&member);
            component.push(member);
            if member == root {
                break;
            }
        }
        component.reverse();
        self.components.push(component);
    }
}

#[cfg(test)]
mod tests {
    use rustc_hash::{FxHashMap, FxHashSet};

    use super::{DefinitionGraph, DependencyGraph};
    use crate::core::{BindingArena, BindingId, BindingInfo, BindingKind, ModuleId};
    use crate::types::{TypedProgram, TypedSource};

    struct CallGraph {
        bindings: BindingArena,
        edges: FxHashMap<BindingId, Vec<BindingId>>,
        compile_time_code: FxHashSet<BindingId>,
        macro_helpers: FxHashSet<BindingId>,
    }

    impl CallGraph {
        fn new() -> Self {
            Self {
                bindings: BindingArena::default(),
                edges: FxHashMap::default(),
                compile_time_code: FxHashSet::default(),
                macro_helpers: FxHashSet::default(),
            }
        }

        fn compile_time_function(&mut self, name: &str, callees: &[BindingId]) -> BindingId {
            let binding = self.function(name, callees);
            self.compile_time_code.insert(binding);
            binding
        }

        fn macro_helper(&mut self, name: &str) -> BindingId {
            let binding = self.function(name, &[]);
            self.macro_helpers.insert(binding);
            binding
        }

        fn function(&mut self, name: &str, callees: &[BindingId]) -> BindingId {
            let kind = BindingKind::Global(ModuleId::new(0));
            let binding = self.bindings.add(BindingInfo::new(name.into(), kind, None));
            self.edges.insert(binding, callees.to_vec());
            binding
        }

        fn dependencies(self) -> DependencyGraph {
            DependencyGraph {
                edges: self.edges,
                compile_time_only: self.compile_time_code,
                macro_helpers: self.macro_helpers,
            }
        }
    }

    #[test]
    fn a_failed_callee_is_a_direct_dependency() {
        let mut graph = CallGraph::new();
        let last = graph.function("last-step", &[]);
        let caller = graph.function("middle-step", &[last]);
        let failed = FxHashSet::from_iter([last]);
        assert!(
            graph
                .dependencies()
                .directly_depends_on_any(&[caller], &failed)
        );
    }

    #[test]
    fn a_failure_two_calls_away_is_not_a_direct_dependency() {
        let mut graph = CallGraph::new();
        let last = graph.function("last-step", &[]);
        let middle = graph.function("middle-step", &[last]);
        let first = graph.function("first-step", &[middle]);
        let failed = FxHashSet::from_iter([last]);
        assert!(
            !graph
                .dependencies()
                .directly_depends_on_any(&[first], &failed)
        );
    }

    #[test]
    fn reserves_the_definition_list_once_for_every_item() {
        let source = TypedSource::expand("(def rate 3)\n(def total (* rate 2))\n");
        let program = source.program();
        let items = program.modules.iter().flat_map(|module| &module.items);
        let graph = DefinitionGraph::of(program);
        assert_eq!(graph.definitions.capacity(), items.count());
    }

    #[test]
    fn a_macro_helper_that_only_compile_time_code_calls_stays_unchecked() {
        let mut graph = CallGraph::new();
        let helper = graph.macro_helper("record-name");
        graph.compile_time_function("expansion", &[helper]);
        let main = graph.function("main", &[]);
        assert!(graph.dependencies().unchecked(&[main]).contains(&helper));
    }

    #[test]
    fn a_macro_helper_that_runtime_code_calls_is_checked() {
        let mut graph = CallGraph::new();
        let helper = graph.macro_helper("record-name");
        graph.function("unused-caller", &[helper]);
        let main = graph.function("main", &[]);
        assert!(!graph.dependencies().unchecked(&[main]).contains(&helper));
    }

    #[test]
    fn a_macro_helper_that_main_reaches_through_compile_time_code_is_checked() {
        let mut graph = CallGraph::new();
        let helper = graph.macro_helper("record-name");
        let expansion = graph.compile_time_function("expansion", &[helper]);
        let main = graph.compile_time_function("main", &[expansion]);
        assert!(!graph.dependencies().unchecked(&[main]).contains(&helper));
    }

    #[test]
    fn compile_time_code_outside_main_stays_unchecked_when_runtime_code_calls_it() {
        let mut graph = CallGraph::new();
        let expansion = graph.compile_time_function("expansion", &[]);
        graph.function("unused-caller", &[expansion]);
        let main = graph.function("main", &[]);
        assert!(graph.dependencies().unchecked(&[main]).contains(&expansion));
    }

    #[test]
    fn an_empty_program_types_no_prelude_macro_helper() {
        let source = TypedSource::expand(PreludeFixture::EMPTY_PROGRAM);
        let helpers = DefinitionGraph::prelude_macro_helpers(source.program());
        assert!(!helpers.is_empty());
        assert!(PreludeFixture::typed_bindings(&source.typed()).is_disjoint(&helpers));
    }

    #[test]
    fn every_prelude_macro_helper_outside_compile_time_code_type_checks_once() {
        let source = TypedSource::expand(PreludeFixture::EMPTY_PROGRAM);
        let graph = DefinitionGraph::with_every_definition(source.program());
        let typed = PreludeFixture::typed_bindings(&source.check(&graph));
        let unchecked = DefinitionGraph::prelude_macro_helpers(source.program())
            .into_iter()
            .filter(|helper| !graph.dependencies().is_compile_time_only(*helper))
            .find(|helper| !typed.contains(helper));
        assert_eq!(unchecked, None);
    }

    struct PreludeFixture;

    impl PreludeFixture {
        const EMPTY_PROGRAM: &str = "(defn main ())";

        fn typed_bindings(program: &TypedProgram) -> FxHashSet<BindingId> {
            program
                .modules
                .iter()
                .flat_map(|module| &module.definitions)
                .map(|definition| definition.binding)
                .collect()
        }
    }
}
