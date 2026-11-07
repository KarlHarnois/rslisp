use std::rc::Rc;

use super::Program;
use super::result_printer::{GeneratedFunction, ResultPrinter};
use crate::core::{BindingId, Block, BodyItem, Expr, ExprKind, Item, ItemKind, Lambda};
use crate::source::Span;

#[derive(Clone)]
pub struct GeneratedEntry {
    binding: BindingId,
    body: Block,
    span: Span,
    functions: Vec<GeneratedFunction>,
}

impl GeneratedEntry {
    pub(super) fn new(binding: BindingId, body: Block, span: Span) -> Self {
        Self {
            binding,
            body,
            span,
            functions: Vec::new(),
        }
    }

    pub fn printing(&self, printer: &ResultPrinter, printed: &[usize]) -> Self {
        let items = self
            .body
            .items
            .iter()
            .enumerate()
            .map(|(position, item)| match item {
                BodyItem::Expr(expr) if printed.contains(&position) => {
                    BodyItem::Expr(printer.print(expr))
                }
                other => other.clone(),
            });
        Self {
            body: Block {
                items: items.collect(),
            },
            functions: printer.functions().to_vec(),
            ..self.clone()
        }
    }

    fn defines(&self, binding: BindingId) -> bool {
        binding == self.binding
            || self
                .functions
                .iter()
                .any(|function| function.binding() == binding)
    }

    fn items(&self) -> impl Iterator<Item = Item> {
        let functions = self.functions.iter().map(GeneratedFunction::item);
        functions.chain(std::iter::once(self.item()))
    }

    fn item(&self) -> Item {
        let lambda = Lambda {
            params: Vec::new(),
            body: self.body.clone(),
        };
        let holds_no_written_text = self.span.start();
        let value = Expr::new(ExprKind::Fn(Rc::new(lambda)), holds_no_written_text);
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
            |item| !matches!(item.kind, ItemKind::Def(binding, _) if entry.defines(binding)),
        );
        module.items.extend(entry.items());
        self.entry_function = Some(entry.binding);
        self.restore_reached();
    }
}
