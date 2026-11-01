use std::collections::{HashMap, HashSet};

use crate::core::{BindingArena, BindingId, BindingKind, Expr, ExprKind, ItemKind, Leaf};
use crate::expand::Program;
use crate::scope::{FastHashMap, FastHashSet};
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
        let mut definitions = Vec::with_capacity(Self::item_count(program));
        definitions.extend(Self::definitions_in(program));
        let dependencies = DependencyGraph::build(&definitions, &program.bindings);
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

pub struct DependencyGraph {
    edges: HashMap<BindingId, Vec<BindingId>>,
    compile_time_only: HashSet<BindingId>,
}

impl DependencyGraph {
    pub fn build(definitions: &[Definition], bindings: &BindingArena) -> Self {
        let known: HashSet<BindingId> = definitions
            .iter()
            .map(|definition| definition.binding)
            .collect();
        let mut edges = HashMap::new();
        let mut direct_compile_time = HashSet::new();
        for definition in definitions {
            let mut references = References::default();
            references.collect(definition.value, bindings);
            if references.uses_compile_time_code {
                direct_compile_time.insert(definition.binding);
            }
            edges.insert(
                definition.binding,
                references
                    .globals
                    .into_iter()
                    .filter(|global| known.contains(global))
                    .collect(),
            );
        }
        let mut graph = Self {
            edges,
            compile_time_only: direct_compile_time,
        };
        graph.spread_compile_time();
        graph
    }

    pub fn is_compile_time_only(&self, binding: BindingId) -> bool {
        self.compile_time_only.contains(&binding)
    }

    pub fn directly_depends_on_any(
        &self,
        roots: &[BindingId],
        bindings: &HashSet<BindingId>,
    ) -> bool {
        roots
            .iter()
            .filter_map(|root| self.edges.get(root))
            .flatten()
            .any(|dependency| bindings.contains(dependency))
    }

    pub fn reachable_from(&self, roots: &[BindingId]) -> HashSet<BindingId> {
        let mut reached: HashSet<BindingId> = HashSet::new();
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
    edges: &'a HashMap<BindingId, Vec<BindingId>>,
    index: FastHashMap<BindingId, usize>,
    low_link: FastHashMap<BindingId, usize>,
    stack: Vec<BindingId>,
    on_stack: FastHashSet<BindingId>,
    components: Vec<Vec<BindingId>>,
}

impl<'a> StronglyConnected<'a> {
    fn new(edges: &'a HashMap<BindingId, Vec<BindingId>>) -> Self {
        Self {
            edges,
            index: FastHashMap::default(),
            low_link: FastHashMap::default(),
            stack: Vec::new(),
            on_stack: FastHashSet::default(),
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
    use std::collections::{HashMap, HashSet};

    use super::{DefinitionGraph, DependencyGraph};
    use crate::core::{BindingArena, BindingId, BindingInfo, BindingKind, ModuleId};
    use crate::types::TypedSource;

    struct CallGraph {
        bindings: BindingArena,
        edges: HashMap<BindingId, Vec<BindingId>>,
    }

    impl CallGraph {
        fn new() -> Self {
            Self {
                bindings: BindingArena::default(),
                edges: HashMap::new(),
            }
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
                compile_time_only: HashSet::new(),
            }
        }
    }

    #[test]
    fn a_failed_callee_is_a_direct_dependency() {
        let mut graph = CallGraph::new();
        let last = graph.function("last-step", &[]);
        let caller = graph.function("middle-step", &[last]);
        let failed = HashSet::from([last]);
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
        let failed = HashSet::from([last]);
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
}
