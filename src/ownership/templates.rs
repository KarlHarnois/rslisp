use std::collections::HashMap;

use crate::core::{BindingId, ParsedTemplate};
use crate::types::{TypedExpr, TypedExprKind, TypedLeaf, TypedProgram};

pub struct TemplateBorrows {
    templates: HashMap<BindingId, ParsedTemplate>,
}

impl TemplateBorrows {
    pub fn collect(program: &TypedProgram) -> Self {
        let templates = program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .filter_map(|definition| match &definition.value.kind {
                TypedExprKind::Leaf(TypedLeaf::Rust(template)) => {
                    ParsedTemplate::parse(&template.template)
                        .ok()
                        .map(|parsed| (definition.binding, parsed))
                }
                _ => None,
            })
            .collect();
        Self { templates }
    }

    pub fn borrows(&self, callee: &TypedExpr, position: usize) -> bool {
        match &callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Global(binding)) => self
                .templates
                .get(binding)
                .is_some_and(|parsed| parsed.borrows(position)),
            TypedExprKind::Leaf(TypedLeaf::Rust(template)) => {
                ParsedTemplate::parse(&template.template)
                    .is_ok_and(|parsed| parsed.borrows(position))
            }
            _ => false,
        }
    }
}
