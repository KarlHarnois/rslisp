use rustc_hash::FxHashMap;

use crate::core::BindingId;
use crate::types::{TypedBodyItem, TypedControl, TypedExpr, TypedExprKind, TypedLambda, TypedLeaf};

pub struct LetAliases<'e> {
    values: FxHashMap<BindingId, &'e TypedExpr>,
}

pub struct LetAlias<'e> {
    pub alias: BindingId,
    pub target: BindingId,
    pub bound: &'e TypedExpr,
}

impl<'e> LetAliases<'e> {
    pub fn in_expressions(expressions: &[&'e TypedExpr]) -> Self {
        Self {
            values: expressions
                .iter()
                .flat_map(|expr| Self::body_items(expr))
                .filter_map(|item| match item {
                    TypedBodyItem::Let(binder, value) => Some((binder.binding, value)),
                    TypedBodyItem::Expr(_) => None,
                })
                .collect(),
        }
    }

    pub fn unannotated_in(value: &'e TypedExpr) -> Self {
        Self {
            values: value
                .let_bindings()
                .filter(|(binder, _)| !binder.annotated)
                .map(|(binder, bound)| (binder.binding, bound))
                .collect(),
        }
    }

    pub fn functions_in(expressions: &[&'e TypedExpr]) -> Vec<(BindingId, &'e TypedLambda)> {
        expressions
            .iter()
            .flat_map(|expr| Self::body_items(expr))
            .filter_map(|item| match item {
                TypedBodyItem::Let(binder, value) => match &value.kind {
                    TypedExprKind::Fn(lambda) => Some((binder.binding, lambda.as_ref())),
                    _ => None,
                },
                TypedBodyItem::Expr(_) => None,
            })
            .collect()
    }

    fn body_items(expr: &'e TypedExpr) -> &'e [TypedBodyItem] {
        match &expr.kind {
            TypedExprKind::Fn(lambda) => &lambda.body.items,
            TypedExprKind::Control(TypedControl::Block(block)) => &block.items,
            _ => &[],
        }
    }

    pub fn resolve(&self, value: &'e TypedExpr) -> &'e TypedExpr {
        let mut resolved = value;
        while let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = resolved.kind
            && let Some(bound) = self.values.get(&binding)
        {
            resolved = bound;
        }
        resolved
    }

    pub fn entries(&self) -> impl Iterator<Item = (BindingId, &'e TypedExpr)> + '_ {
        self.values
            .iter()
            .map(|(binding, bound)| (*binding, *bound))
    }

    pub fn links(&self, alias: BindingId) -> Vec<LetAlias<'e>> {
        let mut links = Vec::new();
        let mut current = alias;
        while let Some(bound) = self.values.get(&current)
            && let TypedExprKind::Leaf(TypedLeaf::Local(target)) = bound.kind
        {
            links.push(LetAlias {
                alias: current,
                target,
                bound,
            });
            current = target;
        }
        links
    }

    pub fn origins(&self) -> FxHashMap<BindingId, BindingId> {
        self.values
            .keys()
            .map(|alias| (*alias, self.origin(*alias)))
            .filter(|(alias, origin)| alias != origin)
            .collect()
    }

    fn origin(&self, alias: BindingId) -> BindingId {
        let mut origin = alias;
        while let Some(TypedExprKind::Leaf(TypedLeaf::Local(bound))) =
            self.values.get(&origin).map(|value| &value.kind)
        {
            origin = *bound;
        }
        origin
    }
}
