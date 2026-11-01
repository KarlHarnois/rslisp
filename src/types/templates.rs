use std::cell::RefCell;
use std::rc::Rc;

use rustc_hash::FxHashMap;

use super::typed::{TypedExpr, TypedExprKind, TypedLeaf};
use crate::core::{BindingId, ParsedTemplate, ReferenceKind};

#[derive(Default)]
pub struct TemplateBorrows {
    templates: FxHashMap<BindingId, ParsedTemplate>,
    inline: RefCell<FxHashMap<Rc<str>, Option<ParsedTemplate>>>,
}

impl TemplateBorrows {
    pub fn register(&mut self, binding: BindingId, template: &str) {
        if let Ok(parsed) = ParsedTemplate::parse(template) {
            self.templates.insert(binding, parsed);
        }
    }

    pub fn borrows(&self, callee: &TypedExpr, position: usize) -> bool {
        self.read_template(callee, false, |parsed| parsed.borrows(position))
    }

    pub fn advances(&self, callee: &TypedExpr, position: usize) -> bool {
        self.read_template(callee, false, |parsed| parsed.advances_receiver(position))
    }

    pub fn receives(&self, callee: &TypedExpr, position: usize) -> bool {
        self.read_template(callee, false, |parsed| parsed.receives(position))
    }

    pub fn keeps(&self, callee: &TypedExpr, position: usize) -> bool {
        self.reference(callee, position).is_some()
    }

    pub fn reference(&self, callee: &TypedExpr, position: usize) -> Option<ReferenceKind> {
        self.read_template(callee, None, |parsed| {
            if parsed.advances_receiver(position) {
                Some(ReferenceKind::Mutable)
            } else if parsed.borrows(position) {
                Some(ReferenceKind::Shared)
            } else {
                None
            }
        })
    }

    fn read_template<T>(
        &self,
        callee: &TypedExpr,
        absent: T,
        read: impl Fn(&ParsedTemplate) -> T,
    ) -> T {
        match &callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Global(binding)) => {
                self.templates.get(binding).map(read).unwrap_or(absent)
            }
            TypedExprKind::Leaf(TypedLeaf::Rust(template)) => {
                let mut inline = self.inline.borrow_mut();
                let parsed = inline
                    .entry(template.template.clone())
                    .or_insert_with(|| ParsedTemplate::parse(&template.template).ok());
                parsed.as_ref().map(read).unwrap_or(absent)
            }
            _ => absent,
        }
    }
}
