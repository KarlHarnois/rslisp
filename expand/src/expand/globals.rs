use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::Expander;
use super::budget_arena::BudgetId;
use super::problems::ExpandProblems;
use super::recur_check::RecurCheck;
use super::try_check::TryCheck;
use crate::core::{BindingId, Expr};
use crate::diagnostic::Diagnostic;
use crate::interpret::{Env, Interpreter, Value};
use crate::reader::Form;
use crate::scope::Phase;
use crate::source::Span;

#[derive(serde::Serialize, serde::Deserialize)]
enum GlobalState {
    Pending { value: Form, budget: BudgetId },
    Lowering,
    Lowered(Rc<Expr>),
    Failed,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct GlobalTable {
    states: FxHashMap<BindingId, GlobalState>,
    values: FxHashMap<BindingId, Value>,
    evaluating: FxHashSet<BindingId>,
    compile_time_checks: FxHashMap<BindingId, CompileTimeCheck>,
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
        match self
            .tables
            .globals
            .states
            .insert(binding, GlobalState::Lowering)
        {
            Some(GlobalState::Pending { value, budget }) => {
                self.spending(budget, |expander| expander.lower_global(binding, &value))
            }
            Some(GlobalState::Lowered(expr)) => {
                self.tables
                    .globals
                    .states
                    .insert(binding, GlobalState::Lowered(Rc::clone(&expr)));
                Ok(expr)
            }
            Some(GlobalState::Failed) => {
                self.tables
                    .globals
                    .states
                    .insert(binding, GlobalState::Failed);
                Err(ExpandProblems::broken_definition(
                    self.tables.bindings.get(binding).name(),
                    span,
                ))
            }
            Some(GlobalState::Lowering) | None => Err(ExpandProblems::compile_time_cycle(
                self.tables.bindings.get(binding).name(),
                span,
            )),
        }
    }

    pub(super) fn evaluate_global(
        &mut self,
        binding: BindingId,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        if let Some(value) = self.tables.globals.values.get(&binding) {
            return Ok(value.clone());
        }
        if !self.tables.globals.evaluating.insert(binding) {
            return Err(ExpandProblems::compile_time_cycle(
                self.tables.bindings.get(binding).name(),
                span,
            ));
        }
        let result = self
            .lowered_global(binding, span)
            .and_then(|expr| Interpreter::new(self, span).eval(&expr, &Env::default()));
        self.tables.globals.evaluating.remove(&binding);
        let value = result?;
        self.tables.globals.values.insert(binding, value.clone());
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
                    self.tables
                        .globals
                        .states
                        .insert(binding, GlobalState::Failed);
                    return Err(self.recovered_diagnostics.remove(errors_before));
                }
                RecurCheck::outside().value(&expr)?;
                TryCheck::outside().value(&expr)?;
                let expr = Rc::new(expr);
                self.tables
                    .globals
                    .states
                    .insert(binding, GlobalState::Lowered(Rc::clone(&expr)));
                Ok(expr)
            }
            Err(diagnostic) => {
                self.tables
                    .globals
                    .states
                    .insert(binding, GlobalState::Failed);
                Err(diagnostic)
            }
        }
    }
}
