use std::rc::Rc;

use super::Program;
use crate::core::{BindingId, Block, BodyItem, Expr, ExprKind, Item, ItemKind, Lambda, Leaf};
use crate::source::Span;

#[derive(Clone)]
pub struct GeneratedEntry {
    binding: BindingId,
    body: Block,
    forms: Vec<WrittenForm>,
    span: Span,
}

#[derive(Clone, Debug)]
pub struct WrittenForm {
    pub span: Span,
    pub text: Rc<str>,
}

impl GeneratedEntry {
    pub(super) fn new(
        binding: BindingId,
        body: Block,
        forms: Vec<WrittenForm>,
        span: Span,
    ) -> Self {
        Self {
            binding,
            body,
            forms,
            span,
        }
    }

    pub fn forms(&self) -> &[WrittenForm] {
        &self.forms
    }

    pub fn printing(&self, printer: BindingId, printed: &[usize]) -> Self {
        let items = self
            .body
            .items
            .iter()
            .enumerate()
            .map(|(position, item)| match item {
                BodyItem::Expr(expr) if printed.contains(&position) => {
                    BodyItem::Expr(Self::printed(printer, expr))
                }
                other => other.clone(),
            });
        Self {
            body: Block {
                items: items.collect(),
            },
            ..self.clone()
        }
    }

    fn printed(printer: BindingId, expr: &Expr) -> Expr {
        let callee = Expr::new(ExprKind::Leaf(Leaf::Var(printer)), expr.span());
        Expr::new(
            ExprKind::Call(Box::new(callee), vec![expr.clone()]),
            expr.span(),
        )
    }

    fn item(&self) -> Item {
        let lambda = Lambda {
            params: Vec::new(),
            body: self.body.clone(),
        };
        let value = Expr::new(ExprKind::Fn(Rc::new(lambda)), self.span);
        Item {
            kind: ItemKind::Def(self.binding, Rc::new(value)),
            span: self.span,
            hidden: false,
        }
    }
}

impl Program {
    pub fn enter(&mut self, entry: &GeneratedEntry) {
        let entry_module = self.entry;
        let module = self
            .modules
            .iter_mut()
            .find(|module| module.id == entry_module)
            .expect("the entry module is expanded");
        module.items.retain(
            |item| !matches!(item.kind, ItemKind::Def(binding, _) if binding == entry.binding),
        );
        module.items.push(entry.item());
        self.entry_function = Some(entry.binding);
        self.evaluated_forms = entry.forms.clone();
    }
}
