use std::collections::HashSet;

use crate::core::BindingId;
use crate::types::{
    TypedBlock, TypedBodyItem, TypedControl, TypedExpr, TypedExprKind, TypedLeaf, TypedProgram,
};

pub struct EscapingLocals {
    let_bound: HashSet<BindingId>,
    escaping: HashSet<BindingId>,
}

impl EscapingLocals {
    pub fn in_program(program: &TypedProgram) -> HashSet<BindingId> {
        let mut found = Self {
            let_bound: HashSet::new(),
            escaping: HashSet::new(),
        };
        program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .for_each(|definition| found.expr(&definition.value));
        found.escaping
    }

    fn expr(&mut self, expr: &TypedExpr) {
        match &expr.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => self.use_as_value(*binding),
            TypedExprKind::Call(callee, arguments) if Self::is_local(callee) => arguments
                .iter()
                .for_each(|argument| self.expr(&argument.value)),
            TypedExprKind::Fn(lambda) => self.block(&lambda.body),
            TypedExprKind::Control(TypedControl::Block(block)) => self.block(block),
            TypedExprKind::Control(TypedControl::Match(matched)) => {
                self.expr(&matched.value);
                matched.arms.iter().for_each(|arm| self.block(&arm.body));
            }
            _ => expr
                .children()
                .into_iter()
                .for_each(|child| self.expr(child)),
        }
    }

    fn block(&mut self, block: &TypedBlock) {
        for item in &block.items {
            if let TypedBodyItem::Let(binder, _) = item {
                self.let_bound.insert(binder.binding);
            }
            self.expr(item.expr());
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
