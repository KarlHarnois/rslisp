use std::collections::HashSet;

use crate::core::{BindingId, ReferenceKind};
use crate::ownership::ChangedValues;
use crate::types::{
    Borrow, TypedArgument, TypedBlock, TypedBodyItem, TypedControl, TypedExpr, TypedExprKind,
    TypedLeaf,
};

#[derive(Default)]
pub struct MutableLocals {
    bindings: HashSet<BindingId>,
    mutating_closures: HashSet<BindingId>,
    called_closures: HashSet<BindingId>,
}

impl MutableLocals {
    pub fn of(value: &TypedExpr) -> Self {
        let mut found = Self::default();
        found.expr(value);
        let called_closures: Vec<BindingId> = found
            .mutating_closures
            .intersection(&found.called_closures)
            .copied()
            .collect();
        found.bindings.extend(called_closures);
        found
    }

    pub fn extend(&mut self, bindings: impl IntoIterator<Item = BindingId>) {
        self.bindings.extend(bindings);
    }

    pub fn into_set(self) -> HashSet<BindingId> {
        self.bindings
    }

    fn expr(&mut self, expr: &TypedExpr) {
        match &expr.kind {
            TypedExprKind::Fn(lambda) => self.block(&lambda.body),
            TypedExprKind::Control(control) => self.control(control),
            TypedExprKind::Call(callee, arguments) => self.call(callee, arguments),
            TypedExprKind::Recur(arguments) => self.arguments(arguments),
            TypedExprKind::Borrow(kind, value) => self.borrow(*kind, value),
            _ => self.children(expr),
        }
    }

    fn block(&mut self, block: &TypedBlock) {
        for item in &block.items {
            if let TypedBodyItem::Let(binder, value) = item
                && Self::mutates_capture(binder.binding, value)
            {
                self.mutating_closures.insert(binder.binding);
            }
            self.expr(item.expr());
        }
    }

    fn control(&mut self, control: &TypedControl) {
        match control {
            TypedControl::Block(block) => self.block(block),
            TypedControl::If(test, then, otherwise) => {
                self.expr(test);
                self.expr(then);
                self.expr(otherwise);
            }
            TypedControl::Match(matched) => {
                self.expr(&matched.value);
                for arm in &matched.arms {
                    self.block(&arm.body);
                }
            }
        }
    }

    fn call(&mut self, callee: &TypedExpr, arguments: &[TypedArgument]) {
        self.record_call(callee);
        self.arguments(arguments);
        self.expr(callee);
    }

    fn borrow(&mut self, kind: ReferenceKind, value: &TypedExpr) {
        if kind == ReferenceKind::Mutable {
            self.record_mutable_place(value);
        }
        self.expr(value);
    }

    fn children(&mut self, expr: &TypedExpr) {
        expr.children()
            .into_iter()
            .for_each(|child| self.expr(child));
    }

    fn record_call(&mut self, callee: &TypedExpr) {
        if let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = callee.kind {
            self.called_closures.insert(binding);
        }
    }

    fn arguments(&mut self, arguments: &[TypedArgument]) {
        for argument in arguments {
            if argument.borrow == Borrow::Mutable {
                self.record_mutable_place(&argument.value);
            }
            self.expr(&argument.value);
        }
    }

    fn record_mutable_place(&mut self, value: &TypedExpr) {
        if let Some(binding) = value.place_root() {
            self.bindings.insert(binding);
        }
    }

    fn mutates_capture(enclosing: BindingId, value: &TypedExpr) -> bool {
        let TypedExprKind::Fn(lambda) = &value.kind else {
            return false;
        };
        let locals: HashSet<BindingId> = value
            .binders()
            .iter()
            .map(|binder| binder.binding)
            .collect();
        ChangedValues::of(lambda, enclosing)
            .changed_bindings()
            .any(|binding| !locals.contains(&binding))
    }
}
