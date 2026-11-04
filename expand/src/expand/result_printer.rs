use std::rc::Rc;

use crate::core::{BindingId, Expr, ExprKind, Item, ItemKind, Leaf};
use crate::source::Span;

#[derive(Clone)]
pub struct GeneratedFunction {
    binding: BindingId,
    value: Rc<Expr>,
    span: Span,
}

#[derive(Clone)]
pub struct ResultPrinter {
    callee: BindingId,
    bound_holder: BindingId,
    functions: Vec<GeneratedFunction>,
}
impl GeneratedFunction {
    pub(super) fn new(binding: BindingId, value: Expr) -> Self {
        Self {
            binding,
            span: value.span(),
            value: Rc::new(value),
        }
    }

    pub(super) fn binding(&self) -> BindingId {
        self.binding
    }

    pub(super) fn item(&self) -> Item {
        Item {
            kind: ItemKind::Def(self.binding, Rc::clone(&self.value)),
            span: self.span,
            hidden: false,
        }
    }
}

impl ResultPrinter {
    pub fn calling(printer: BindingId) -> Self {
        Self {
            callee: printer,
            bound_holder: printer,
            functions: Vec::new(),
        }
    }

    pub fn through(function: GeneratedFunction, bound_holder: BindingId) -> Self {
        Self {
            callee: function.binding,
            bound_holder,
            functions: vec![function],
        }
    }

    pub fn bound_holder(&self) -> BindingId {
        self.bound_holder
    }

    pub(super) fn functions(&self) -> &[GeneratedFunction] {
        &self.functions
    }

    pub(super) fn print(&self, value: &Expr) -> Expr {
        let span = value.span();
        let callee = Expr::new(ExprKind::Leaf(Leaf::Var(self.callee)), span);
        Expr::new(ExprKind::Call(Box::new(callee), vec![value.clone()]), span)
    }
}
