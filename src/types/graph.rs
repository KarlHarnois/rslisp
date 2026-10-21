use std::collections::{HashMap, HashSet};

use crate::core::{BindingArena, BindingId, BindingKind, Expr, ExprKind, Leaf};

pub struct DependencyGraph {
    edges: HashMap<BindingId, Vec<BindingId>>,
    compile_time_only: HashSet<BindingId>,
}

impl DependencyGraph {
    pub fn build(definitions: &[(BindingId, &Expr)], bindings: &BindingArena) -> Self {
        let known: HashSet<BindingId> = definitions.iter().map(|(binding, _)| *binding).collect();
        let mut edges = HashMap::new();
        let mut direct_compile_time = HashSet::new();
        for (binding, value) in definitions {
            let mut references = References::default();
            references.collect(value, bindings);
            if references.uses_compile_time_code {
                direct_compile_time.insert(*binding);
            }
            edges.insert(
                *binding,
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
    index: HashMap<BindingId, usize>,
    low_link: HashMap<BindingId, usize>,
    stack: Vec<BindingId>,
    on_stack: HashSet<BindingId>,
    components: Vec<Vec<BindingId>>,
}

impl<'a> StronglyConnected<'a> {
    fn new(edges: &'a HashMap<BindingId, Vec<BindingId>>) -> Self {
        Self {
            edges,
            index: HashMap::new(),
            low_link: HashMap::new(),
            stack: Vec::new(),
            on_stack: HashSet::new(),
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
