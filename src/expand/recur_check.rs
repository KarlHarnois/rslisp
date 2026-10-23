use super::problems::ExpandProblems;
use crate::core::{Block, BodyItem, Control, Expr, ExprKind, StructExpr};
use crate::diagnostic::Diagnostic;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TailPosition {
    Tail,
    NotTail,
}

pub struct RecurCheck {
    parameters: Option<usize>,
}

impl RecurCheck {
    pub fn inside(parameters: usize) -> Self {
        Self {
            parameters: Some(parameters),
        }
    }

    pub fn outside() -> Self {
        Self { parameters: None }
    }

    pub fn block(&self, block: &Block) -> Result<(), Diagnostic> {
        self.items(&block.items, TailPosition::Tail)
    }

    pub fn value(&self, expr: &Expr) -> Result<(), Diagnostic> {
        self.expr(expr, TailPosition::Tail)
    }

    fn items(&self, items: &[BodyItem], position: TailPosition) -> Result<(), Diagnostic> {
        let last = items.len().checked_sub(1);
        items
            .iter()
            .enumerate()
            .try_for_each(|(index, item)| match item {
                BodyItem::Let(_, value) => self.expr(value, TailPosition::NotTail),
                BodyItem::Expr(expr) if Some(index) == last => self.expr(expr, position),
                BodyItem::Expr(expr) => self.expr(expr, TailPosition::NotTail),
            })
    }

    fn expr(&self, expr: &Expr, position: TailPosition) -> Result<(), Diagnostic> {
        match expr.kind() {
            ExprKind::Leaf(_) | ExprKind::Fn(_) => Ok(()),
            ExprKind::Call(callee, arguments) => {
                self.expr(callee, TailPosition::NotTail)?;
                self.arguments(arguments.iter())
            }
            ExprKind::Control(control) => self.control(control, position),
            other => self.nested_expr(expr, other, position),
        }
    }

    fn nested_expr(
        &self,
        expr: &Expr,
        kind: &ExprKind,
        position: TailPosition,
    ) -> Result<(), Diagnostic> {
        match kind {
            ExprKind::Recur(arguments) => self.recur(expr, arguments, position),
            ExprKind::Borrow(_, value) | ExprKind::Deref(value) => {
                self.expr(value, TailPosition::NotTail)
            }
            ExprKind::Struct(structure) => self.structure(structure),
            ExprKind::Variant(_, arguments) => self.arguments(arguments.iter()),
            _ => unreachable!("the caller matches a nested expression"),
        }
    }

    fn control(&self, control: &Control, position: TailPosition) -> Result<(), Diagnostic> {
        match control {
            Control::Block(block) => self.items(&block.items, position),
            Control::If(test, then, otherwise) => self.branches(test, then, otherwise, position),
        }
    }

    fn branches(
        &self,
        test: &Expr,
        then: &Expr,
        otherwise: &Expr,
        position: TailPosition,
    ) -> Result<(), Diagnostic> {
        self.expr(test, TailPosition::NotTail)?;
        self.expr(then, position)?;
        self.expr(otherwise, position)
    }

    fn structure(&self, structure: &StructExpr) -> Result<(), Diagnostic> {
        match structure {
            StructExpr::New(_, fields) => self.arguments(fields.iter().map(|(_, value)| value)),
            StructExpr::Field(value, _) => self.expr(value, TailPosition::NotTail),
        }
    }

    fn arguments<'e>(
        &self,
        arguments: impl IntoIterator<Item = &'e Expr>,
    ) -> Result<(), Diagnostic> {
        arguments
            .into_iter()
            .try_for_each(|argument| self.expr(argument, TailPosition::NotTail))
    }

    fn recur(
        &self,
        expr: &Expr,
        arguments: &[Expr],
        position: TailPosition,
    ) -> Result<(), Diagnostic> {
        let Some(parameters) = self.parameters else {
            return Err(ExpandProblems::recur_outside_fn(expr.span()));
        };
        if position == TailPosition::NotTail {
            return Err(ExpandProblems::recur_outside_tail(expr.span()));
        }
        if parameters != arguments.len() {
            return Err(ExpandProblems::recur_argument_count(
                parameters,
                arguments.len(),
                expr.span(),
            ));
        }
        self.arguments(arguments.iter())
    }
}
