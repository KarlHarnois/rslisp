use std::collections::{HashMap, HashSet};

use crate::core::{BindingId, ReferenceKind};
use crate::types::{
    Borrow, TypedArgument, TypedBlock, TypedBodyItem, TypedControl, TypedExpr, TypedExprKind,
    TypedLambda, TypedLeaf, TypedStructExpr,
};

#[derive(Default)]
pub struct ChangedValues {
    aliases: HashMap<BindingId, BindingId>,
    changed: HashSet<BindingId>,
    recursive_arguments: HashSet<BindingId>,
}

impl ChangedValues {
    pub fn of(lambda: &TypedLambda, enclosing: BindingId) -> Self {
        let mut values = Self {
            aliases: HashMap::new(),
            changed: HashSet::new(),
            recursive_arguments: HashSet::new(),
        };
        values.block(&lambda.body, enclosing);
        values
    }

    pub fn is_changed(&self, binding: BindingId) -> bool {
        self.changed.contains(&self.root(binding))
    }

    pub fn changed_bindings(&self) -> impl Iterator<Item = BindingId> + '_ {
        self.changed.iter().copied()
    }

    pub fn is_recursive_argument(&self, binding: BindingId) -> bool {
        self.recursive_arguments.contains(&binding)
    }

    fn block(&mut self, block: &TypedBlock, enclosing: BindingId) {
        for item in &block.items {
            match item {
                TypedBodyItem::Let(binder, value) => {
                    if let TypedExprKind::Leaf(TypedLeaf::Local(source)) = &value.kind {
                        self.aliases.entry(binder.binding).or_insert(*source);
                    }
                    self.expr(value, enclosing);
                }
                TypedBodyItem::Expr(expr) => self.expr(expr, enclosing),
            }
        }
    }

    fn expr(&mut self, expr: &TypedExpr, enclosing: BindingId) {
        match &expr.kind {
            TypedExprKind::Call(callee, arguments) => {
                self.call(callee, arguments, enclosing);
            }
            TypedExprKind::Control(control) => self.control(control, enclosing),
            other => self.nested_expr(expr, other, enclosing),
        }
    }

    fn nested_expr(&mut self, expr: &TypedExpr, kind: &TypedExprKind, enclosing: BindingId) {
        match kind {
            TypedExprKind::Recur(arguments) => self.arguments(arguments, enclosing, true),
            TypedExprKind::Struct(structure) => self.structure(structure, enclosing),
            TypedExprKind::Variant(_, arguments) => self.expressions(arguments, enclosing),
            TypedExprKind::Assoc(value, keys, item) => self.assoc(value, keys, item, enclosing),
            TypedExprKind::Fn(lambda) => self.block(&lambda.body, enclosing),
            _ => self.value_kind(expr, kind, enclosing),
        }
    }

    fn value_kind(&mut self, expr: &TypedExpr, kind: &TypedExprKind, enclosing: BindingId) {
        match kind {
            TypedExprKind::Try(value) => self.expr(value, enclosing),
            TypedExprKind::Borrow(..) | TypedExprKind::Deref(_) => self.written(expr, enclosing),
            _ => {}
        }
    }

    fn expressions(&mut self, arguments: &[TypedExpr], enclosing: BindingId) {
        arguments
            .iter()
            .for_each(|argument| self.expr(argument, enclosing));
    }

    fn assoc(
        &mut self,
        value: &TypedExpr,
        keys: &[TypedExpr],
        item: &TypedExpr,
        enclosing: BindingId,
    ) {
        self.expr(value, enclosing);
        self.expressions(keys, enclosing);
        self.expr(item, enclosing);
    }

    fn written(&mut self, expr: &TypedExpr, enclosing: BindingId) {
        match &expr.kind {
            TypedExprKind::Borrow(ReferenceKind::Mutable, value) => {
                if let Some(place) = value.place_root() {
                    let root = self.root(place);
                    self.changed.insert(root);
                }
                self.expr(value, enclosing);
            }
            TypedExprKind::Borrow(ReferenceKind::Shared, value) | TypedExprKind::Deref(value) => {
                self.expr(value, enclosing);
            }
            _ => unreachable!("the caller matches a written form"),
        }
    }

    fn call(&mut self, callee: &TypedExpr, arguments: &[TypedArgument], enclosing: BindingId) {
        let recursive = matches!(
            callee.kind,
            TypedExprKind::Leaf(TypedLeaf::Global(binding)) if binding == enclosing
        );
        self.arguments(arguments, enclosing, recursive);
        self.expr(callee, enclosing);
    }

    fn control(&mut self, control: &TypedControl, enclosing: BindingId) {
        match control {
            TypedControl::Block(block) => self.block(block, enclosing),
            TypedControl::If(test, then, otherwise) => {
                self.expr(test, enclosing);
                self.expr(then, enclosing);
                self.expr(otherwise, enclosing);
            }
            TypedControl::Match(matched) => {
                self.expr(&matched.value, enclosing);
                for arm in &matched.arms {
                    self.block(&arm.body, enclosing);
                }
            }
        }
    }

    fn structure(&mut self, structure: &TypedStructExpr, enclosing: BindingId) {
        match structure {
            TypedStructExpr::Field(value, _) => self.expr(value, enclosing),
            TypedStructExpr::New(_, fields) => fields
                .iter()
                .for_each(|(_, value)| self.expr(value, enclosing)),
            TypedStructExpr::Update(_, base, fields) => {
                self.expr(base, enclosing);
                fields
                    .iter()
                    .for_each(|(_, value)| self.expr(value, enclosing));
            }
        }
    }

    fn arguments(&mut self, arguments: &[TypedArgument], enclosing: BindingId, recursive: bool) {
        for argument in arguments {
            if matches!(argument.borrow, Borrow::Mutable | Borrow::Reborrow)
                && let Some(place) = argument.value.place_root()
            {
                let root = self.root(place);
                self.changed.insert(root);
            }
            if recursive
                && let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = &argument.value.kind
            {
                self.recursive_arguments.insert(*binding);
            }
            self.expr(&argument.value, enclosing);
        }
    }

    fn root(&self, binding: BindingId) -> BindingId {
        let mut root = binding;
        while let Some(alias) = self.aliases.get(&root) {
            root = *alias;
        }
        root
    }
}
