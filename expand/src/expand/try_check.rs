use super::problems::ExpandProblems;
use crate::core::{Block, Expr, ExprKind};
use crate::diagnostic::Diagnostic;

pub struct TryCheck {
    inside: bool,
}

impl TryCheck {
    pub fn inside() -> Self {
        Self { inside: true }
    }

    pub fn outside() -> Self {
        Self { inside: false }
    }

    pub fn block(&self, block: &Block) -> Result<(), Diagnostic> {
        block.expressions().try_for_each(|expr| self.expr(expr))
    }

    pub fn value(&self, expr: &Expr) -> Result<(), Diagnostic> {
        self.expr(expr)
    }

    fn expr(&self, expr: &Expr) -> Result<(), Diagnostic> {
        match expr.kind() {
            ExprKind::Fn(lambda) => Self::inside().block(&lambda.body),
            ExprKind::Try(value) => {
                if !self.inside {
                    return Err(ExpandProblems::try_outside_fn(expr.span()));
                }
                self.expr(value)
            }
            _ => expr
                .children()
                .into_iter()
                .try_for_each(|child| self.expr(child)),
        }
    }
}
