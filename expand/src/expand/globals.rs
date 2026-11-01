use std::rc::Rc;

use super::Expander;
use super::budget_arena::BudgetId;
use super::problems::ExpandProblems;
use super::recur_check::RecurCheck;
use super::try_check::TryCheck;
use crate::core::{BindingId, Expr};
use crate::diagnostic::Diagnostic;
use crate::interpret::{Env, Interpreter, Value};
use crate::reader::Form;
use crate::scope::{FastHashMap, FastHashSet, Phase};
use crate::source::Span;

enum GlobalState {
    Pending { value: Form, budget: BudgetId },
    Lowering,
    Lowered(Rc<Expr>),
    Failed,
}

#[derive(Default)]
pub struct GlobalTable {
    states: FastHashMap<BindingId, GlobalState>,
    values: FastHashMap<BindingId, Value>,
    evaluating: FastHashSet<BindingId>,
    compile_time_checks: FastHashMap<BindingId, CompileTimeCheck>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CompileTimeCheck {
    InProgress,
    Passed,
    Failed,
}

impl GlobalTable {
    pub fn add_pending(&mut self, binding: BindingId, value: Form, budget: BudgetId) {
        self.states
            .insert(binding, GlobalState::Pending { value, budget });
    }

    pub fn compile_time_check(&self, binding: BindingId) -> Option<CompileTimeCheck> {
        self.compile_time_checks.get(&binding).copied()
    }

    pub fn record_compile_time_check(&mut self, binding: BindingId, check: CompileTimeCheck) {
        self.compile_time_checks.insert(binding, check);
    }

    pub fn has_failed(&self, binding: BindingId) -> bool {
        matches!(self.states.get(&binding), Some(GlobalState::Failed))
    }
}

impl Expander<'_> {
    pub(super) fn lowered_global(
        &mut self,
        binding: BindingId,
        span: Span,
    ) -> Result<Rc<Expr>, Diagnostic> {
        match self.globals.states.insert(binding, GlobalState::Lowering) {
            Some(GlobalState::Pending { value, budget }) => {
                self.spending(budget, |expander| expander.lower_global(binding, &value))
            }
            Some(GlobalState::Lowered(expr)) => {
                self.globals
                    .states
                    .insert(binding, GlobalState::Lowered(Rc::clone(&expr)));
                Ok(expr)
            }
            Some(GlobalState::Failed) => {
                self.globals.states.insert(binding, GlobalState::Failed);
                Err(ExpandProblems::broken_definition(
                    self.bindings.get(binding).name(),
                    span,
                ))
            }
            Some(GlobalState::Lowering) | None => Err(ExpandProblems::compile_time_cycle(
                self.bindings.get(binding).name(),
                span,
            )),
        }
    }

    pub(super) fn evaluate_global(
        &mut self,
        binding: BindingId,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        if let Some(value) = self.globals.values.get(&binding) {
            return Ok(value.clone());
        }
        if !self.globals.evaluating.insert(binding) {
            return Err(ExpandProblems::compile_time_cycle(
                self.bindings.get(binding).name(),
                span,
            ));
        }
        let result = self
            .lowered_global(binding, span)
            .and_then(|expr| Interpreter::new(self, span).eval(&expr, &Env::default()));
        self.globals.evaluating.remove(&binding);
        let value = result?;
        self.globals.values.insert(binding, value.clone());
        Ok(value)
    }

    fn lower_global(&mut self, binding: BindingId, form: &Form) -> Result<Rc<Expr>, Diagnostic> {
        let outer_phase = std::mem::replace(&mut self.phase, Phase::Runtime);
        let errors_before = self.recovered_diagnostics.len();
        let lowered = self.lower_expr(form);
        self.phase = outer_phase;
        match lowered {
            Ok(expr) => {
                if self.recovered_diagnostics.len() > errors_before {
                    self.globals.states.insert(binding, GlobalState::Failed);
                    return Err(self.recovered_diagnostics.remove(errors_before));
                }
                RecurCheck::outside().value(&expr)?;
                TryCheck::outside().value(&expr)?;
                let expr = Rc::new(expr);
                self.globals
                    .states
                    .insert(binding, GlobalState::Lowered(Rc::clone(&expr)));
                Ok(expr)
            }
            Err(diagnostic) => {
                self.globals.states.insert(binding, GlobalState::Failed);
                Err(diagnostic)
            }
        }
    }
}
