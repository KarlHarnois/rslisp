use super::Expander;
use super::globals::CompileTimeCheck;
use super::problems::ExpandProblems;
use crate::core::{BindingId, BindingKind, Expr, ExprKind, Lambda, Leaf};
use crate::diagnostic::{Diagnostic, Label};
use crate::interpret::InterpretProblems;
use crate::source::Span;

impl Expander<'_> {
    pub(super) fn check_compile_time(&mut self, lambda: &Lambda) -> Result<(), Diagnostic> {
        for expr in lambda.body.expressions() {
            self.check_expr(expr, None)?;
        }
        Ok(())
    }

    fn check_expr(&mut self, expr: &Expr, through: Option<Span>) -> Result<(), Diagnostic> {
        self.check_node(expr, through)?;
        for child in expr.children() {
            self.check_expr(child, through)?;
        }
        Ok(())
    }

    fn check_node(&mut self, expr: &Expr, through: Option<Span>) -> Result<(), Diagnostic> {
        match expr.kind() {
            ExprKind::Leaf(Leaf::Var(binding)) => self.check_global(*binding, expr.span(), through),
            ExprKind::Leaf(Leaf::Rust(_)) => {
                let problem = match self.macro_origins.get(&expr.span()) {
                    Some(origin) => ExpandProblems::macro_program_only(
                        &origin.name,
                        expr.span(),
                        origin.defined,
                    ),
                    None => InterpretProblems::program_only(expr.span()),
                };
                Err(Self::routed(problem, through))
            }
            ExprKind::Struct(_) => Err(Self::routed(
                InterpretProblems::runtime_only("structs", expr.span()),
                through,
            )),
            _ => Ok(()),
        }
    }

    fn check_global(
        &mut self,
        binding: BindingId,
        span: Span,
        through: Option<Span>,
    ) -> Result<(), Diagnostic> {
        if !matches!(self.kind(binding), BindingKind::Global(_)) {
            return Ok(());
        }
        let value = self.lowered_global(binding, span)?;
        let name = self.bindings.get(binding).name().to_string();
        if let ExprKind::Leaf(Leaf::Rust(_)) = value.kind() {
            let problem = ExpandProblems::program_only(&name, span, value.span());
            return Err(Self::routed(problem, through));
        }
        match self.globals.compile_time_check(binding) {
            Some(CompileTimeCheck::InProgress | CompileTimeCheck::Passed) => Ok(()),
            Some(CompileTimeCheck::Failed) => Err(ExpandProblems::broken_definition(&name, span)),
            None => self.check_helper(binding, &value, through.or(Some(span))),
        }
    }

    fn check_helper(
        &mut self,
        binding: BindingId,
        value: &Expr,
        through: Option<Span>,
    ) -> Result<(), Diagnostic> {
        self.globals
            .record_compile_time_check(binding, CompileTimeCheck::InProgress);
        let checked = self.check_expr(value, through);
        let outcome = if checked.is_ok() {
            CompileTimeCheck::Passed
        } else {
            CompileTimeCheck::Failed
        };
        self.globals.record_compile_time_check(binding, outcome);
        checked
    }

    fn routed(problem: Diagnostic, through: Option<Span>) -> Diagnostic {
        match through {
            Some(span) => {
                problem.with_label(Label::new(span, "macro code reaches it through this"))
            }
            None => problem,
        }
    }
}
