use std::collections::HashSet;

use crate::core::BindingId;
use crate::types::{TypedExpr, TypedExprKind, TypedLeaf, TypedProgram};

pub struct EscapingLambdas {
    let_bound_lambdas: HashSet<BindingId>,
    escaping: HashSet<BindingId>,
}

impl EscapingLambdas {
    pub fn in_program(program: &TypedProgram) -> HashSet<BindingId> {
        let definitions = || {
            program
                .modules
                .iter()
                .flat_map(|module| &module.definitions)
        };
        let mut found = Self {
            let_bound_lambdas: definitions()
                .flat_map(|definition| definition.value.let_bindings())
                .filter(|(_, value)| value.lambda_value().is_some())
                .map(|(binder, _)| binder.binding)
                .collect(),
            escaping: HashSet::new(),
        };
        if !found.let_bound_lambdas.is_empty() {
            definitions().for_each(|definition| found.expr(&definition.value));
        }
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
        if self.let_bound_lambdas.contains(&binding) {
            self.escaping.insert(binding);
        }
    }

    fn is_local(callee: &TypedExpr) -> bool {
        matches!(callee.kind, TypedExprKind::Leaf(TypedLeaf::Local(_)))
    }
}
