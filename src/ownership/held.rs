use std::collections::HashSet;

use super::aliases::LetAliases;
use crate::core::BindingId;
use crate::types::{
    TypedBlock, TypedBodyItem, TypedControl, TypedExpr, TypedExprKind, TypedLeaf, TypedProgram,
};

pub struct HeldClosures<'h, 'e> {
    aliases: &'h LetAliases<'e>,
    templates: &'h RustTemplates,
}

impl<'h, 'e> HeldClosures<'h, 'e> {
    pub fn new(aliases: &'h LetAliases<'e>, templates: &'h RustTemplates) -> Self {
        Self { aliases, templates }
    }

    pub fn in_value(&self, value: &'e TypedExpr) -> Vec<&'e TypedExpr> {
        self.leaves_of(value)
            .into_iter()
            .filter(|leaf| matches!(leaf.kind, TypedExprKind::Fn(_)))
            .collect()
    }

    pub fn leaves_of(&self, value: &'e TypedExpr) -> Vec<&'e TypedExpr> {
        let value = self.aliases.resolve(value);
        let held = self.held_values(value);
        if held.is_empty() {
            return vec![value];
        }
        held.into_iter()
            .flat_map(|held| self.leaves_of(held))
            .collect()
    }

    fn held_values(&self, value: &'e TypedExpr) -> Vec<&'e TypedExpr> {
        match &value.kind {
            TypedExprKind::Variant(_, payloads) => payloads.iter().collect(),
            TypedExprKind::Call(callee, arguments) if self.templates.is_template(callee) => {
                arguments
                    .iter()
                    .map(|argument| &argument.value)
                    .filter(|argument| value.ty.contains(&argument.ty))
                    .collect()
            }
            TypedExprKind::Control(control) => Self::branch_results(control),
            _ => Vec::new(),
        }
    }

    fn branch_results(control: &'e TypedControl) -> Vec<&'e TypedExpr> {
        match control {
            TypedControl::Block(block) => Self::tail_of(block).into_iter().collect(),
            TypedControl::If(_, then, otherwise) => vec![then, otherwise],
            TypedControl::Match(matched) => matched
                .arms
                .iter()
                .filter_map(|arm| Self::tail_of(&arm.body))
                .collect(),
        }
    }

    fn tail_of(block: &'e TypedBlock) -> Option<&'e TypedExpr> {
        match block.items.last()? {
            TypedBodyItem::Expr(tail) => Some(tail),
            TypedBodyItem::Let(..) => None,
        }
    }
}

pub struct RustTemplates {
    bindings: HashSet<BindingId>,
}

impl RustTemplates {
    pub fn of(program: &TypedProgram) -> Self {
        Self {
            bindings: program
                .modules
                .iter()
                .flat_map(|module| &module.definitions)
                .filter(|definition| {
                    matches!(
                        definition.value.kind,
                        TypedExprKind::Leaf(TypedLeaf::Rust(_))
                    )
                })
                .map(|definition| definition.binding)
                .collect(),
        }
    }

    fn is_template(&self, callee: &TypedExpr) -> bool {
        match callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Rust(_)) => true,
            TypedExprKind::Leaf(TypedLeaf::Global(function)) => self.bindings.contains(&function),
            _ => false,
        }
    }
}
