use std::collections::HashMap;

use super::typed::{TypedExpr, TypedExprKind, TypedLeaf};
use crate::core::{BindingId, ParsedTemplate};

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
        self.holds(callee, |parsed| parsed.borrows(position))
    }

    pub fn advances(&self, callee: &TypedExpr, position: usize) -> bool {
        self.holds(callee, |parsed| parsed.advances_receiver(position))
    }

    pub fn keeps(&self, callee: &TypedExpr, position: usize) -> bool {
        self.holds(callee, |parsed| {
            parsed.borrows(position) || parsed.advances_receiver(position)
        })
    }

    fn holds(&self, callee: &TypedExpr, fact: impl Fn(&ParsedTemplate) -> bool) -> bool {
        match &callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Global(binding)) => {
                self.templates.get(binding).is_some_and(fact)
            }
            TypedExprKind::Leaf(TypedLeaf::Rust(template)) => {
                ParsedTemplate::parse(&template.template).is_ok_and(|parsed| fact(&parsed))
            }
            _ => false,
        }
    }
}
