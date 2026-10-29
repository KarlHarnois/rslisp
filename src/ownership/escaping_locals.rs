use std::collections::HashSet;

use crate::core::BindingId;
use crate::types::{TypedExpr, TypedExprKind, TypedLeaf, TypedProgram};

pub struct EscapingLocals {
    let_bound: HashSet<BindingId>,
    escaping: HashSet<BindingId>,
}

impl EscapingLocals {
    pub fn in_program(program: &TypedProgram) -> HashSet<BindingId> {
        let definitions = || {
            program
                .modules
                .iter()
                .flat_map(|module| &module.definitions)
        };
        let mut found = Self {
            let_bound: definitions()
                .flat_map(|definition| definition.value.let_bindings())
                .map(|(binder, _)| binder.binding)
                .collect(),
            escaping: HashSet::new(),
        };
        definitions().for_each(|definition| found.expr(&definition.value));
        found.escaping
    }

    fn expr(&mut self, expr: &TypedExpr) {
        match &expr.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => self.use_as_value(*binding),
            TypedExprKind::Call(callee, arguments) if Self::is_local(callee) => arguments
                .iter()
                .for_each(|argument| self.expr(&argument.value)),
            _ => expr
                .children()
                .into_iter()
                .for_each(|child| self.expr(child)),
        }
    }

    fn use_as_value(&mut self, binding: BindingId) {
        if self.let_bound.contains(&binding) {
            self.escaping.insert(binding);
        }
    }

    fn is_local(callee: &TypedExpr) -> bool {
        matches!(callee.kind, TypedExprKind::Leaf(TypedLeaf::Local(_)))
    }
}
