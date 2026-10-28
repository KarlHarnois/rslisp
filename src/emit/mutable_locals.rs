use std::collections::HashSet;

use crate::core::{BindingId, ReferenceKind};
use crate::ownership::ChangedValues;
use crate::types::{
    Borrow, TemplateBorrows, TypedArgument, TypedBlock, TypedBodyItem, TypedControl, TypedExpr,
    TypedExprKind, TypedLambda, TypedLeaf,
};

pub struct MutableLocals<'a> {
    templates: &'a TemplateBorrows,
    bindings: HashSet<BindingId>,
    mutating_closures: HashSet<BindingId>,
    called_closures: HashSet<BindingId>,
}

impl<'a> MutableLocals<'a> {
    pub fn of(value: &TypedExpr, templates: &'a TemplateBorrows) -> Self {
        let mut found = Self {
            templates,
            bindings: HashSet::new(),
            mutating_closures: HashSet::new(),
            called_closures: HashSet::new(),
        };
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
            match item {
                TypedBodyItem::Let(binder, value) => self.let_value(binder.binding, value),
                _ => self.expr(item.expr()),
            }
        }
    }

    fn let_value(&mut self, binding: BindingId, value: &TypedExpr) {
        match &value.kind {
            TypedExprKind::Fn(lambda) => self.let_closure(binding, value, lambda),
            _ => self.expr(value),
        }
    }

    fn let_closure(&mut self, binding: BindingId, closure: &TypedExpr, lambda: &TypedLambda) {
        let inside = MutableLocals::of(closure, self.templates);
        if inside.changes_capture(binding, closure, lambda) {
            self.mutating_closures.insert(binding);
        }
        self.absorb(inside);
    }

    fn absorb(&mut self, inside: MutableLocals) {
        self.bindings.extend(inside.bindings);
        self.mutating_closures.extend(inside.mutating_closures);
        self.called_closures.extend(inside.called_closures);
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
        self.record_advanced_locals(callee, arguments);
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

    fn record_advanced_locals(&mut self, callee: &TypedExpr, arguments: &[TypedArgument]) {
        for (position, argument) in arguments.iter().enumerate() {
            if let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = argument.value.kind
                && !argument.value.ty.is_reference()
                && self.templates.advances(callee, position)
            {
                self.bindings.insert(binding);
            }
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

    fn changes_capture(
        &self,
        enclosing: BindingId,
        closure: &TypedExpr,
        lambda: &TypedLambda,
    ) -> bool {
        let locals: HashSet<BindingId> = closure
            .binders()
            .iter()
            .map(|binder| binder.binding)
            .collect();
        ChangedValues::of(lambda, enclosing)
            .changed_bindings()
            .chain(self.bindings.iter().copied())
            .any(|binding| !locals.contains(&binding))
    }
}
