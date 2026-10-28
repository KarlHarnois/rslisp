use std::collections::HashMap;

use super::typed::{TypedExpr, TypedExprKind, TypedLeaf};
use crate::core::{BindingId, ParsedTemplate, ReferenceKind};

#[derive(Default)]
pub struct TemplateBorrows {
    templates: HashMap<BindingId, ParsedTemplate>,
}

impl TemplateBorrows {
    pub fn register(&mut self, binding: BindingId, template: &str) {
        if let Ok(parsed) = ParsedTemplate::parse(template) {
            self.templates.insert(binding, parsed);
        }
    }

    pub fn borrows(&self, callee: &TypedExpr, position: usize) -> bool {
        self.fact(callee, false, |parsed| parsed.borrows(position))
    }

    pub fn advances(&self, callee: &TypedExpr, position: usize) -> bool {
        self.fact(callee, false, |parsed| parsed.advances_receiver(position))
    }

    pub fn keeps(&self, callee: &TypedExpr, position: usize) -> bool {
        self.reference(callee, position).is_some()
    }

    pub fn reference(&self, callee: &TypedExpr, position: usize) -> Option<ReferenceKind> {
        self.fact(callee, None, |parsed| {
            if parsed.advances_receiver(position) {
                Some(ReferenceKind::Mutable)
            } else if parsed.borrows(position) {
                Some(ReferenceKind::Shared)
            } else {
                None
            }
        })
    }

    fn fact<T>(&self, callee: &TypedExpr, absent: T, fact: impl Fn(&ParsedTemplate) -> T) -> T {
        match &callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Global(binding)) => {
                self.templates.get(binding).map(fact).unwrap_or(absent)
            }
            TypedExprKind::Leaf(TypedLeaf::Rust(template)) => {
                ParsedTemplate::parse(&template.template)
                    .as_ref()
                    .map(fact)
                    .unwrap_or(absent)
            }
            _ => absent,
        }
    }
}
