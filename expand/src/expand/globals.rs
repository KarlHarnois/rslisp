use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::Expander;
use super::budget_arena::BudgetId;
use super::problems::ExpandProblems;
use super::recur_check::RecurCheck;
use super::try_check::TryCheck;
use crate::core::{BindingId, Declaration, Expr};
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
    FailedDefinition(Declaration),
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct GlobalTable {
    states: FxHashMap<BindingId, GlobalState>,
    values: FxHashMap<BindingId, Value>,
    evaluating: FxHashSet<BindingId>,
    compile_time_checks: FxHashMap<BindingId, CompileTimeCheck>,
    failed_definition_count: usize,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct LoweredGlobals(Vec<BindingId>);

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

    pub fn macro_helpers(&self) -> FxHashSet<BindingId> {
        self.compile_time_checks
            .iter()
            .filter(|(_, check)| **check == CompileTimeCheck::Passed)
            .map(|(binding, _)| *binding)
            .collect()
    }

    pub fn record_failed_definition(&mut self, binding: BindingId, declaration: Declaration) {
        self.states
            .insert(binding, GlobalState::FailedDefinition(declaration));
        self.failed_definition_count += 1;
    }

    pub fn forget_failed_definition(&mut self, binding: BindingId) {
        self.states.remove(&binding);
        self.failed_definition_count -= 1;
    }

    pub fn has_failed_definitions(&self) -> bool {
        self.failed_definition_count > 0
    }

    pub fn is_failed_macro_definition(&self, binding: BindingId) -> bool {
        matches!(
            self.states.get(&binding),
            Some(GlobalState::FailedDefinition(Declaration::Macro))
        )
    }

    pub fn is_failed_definition(&self, binding: BindingId) -> bool {
        matches!(
            self.states.get(&binding),
            Some(GlobalState::FailedDefinition(_))
        )
    }

    pub fn has_failed(&self, binding: BindingId) -> bool {
        matches!(self.states.get(&binding), Some(GlobalState::Failed))
    }

    pub fn take_lowered<'m>(
        &mut self,
        definitions: impl Iterator<Item = (BindingId, &'m Rc<Expr>)>,
    ) -> LoweredGlobals {
        let states = std::mem::take(&mut self.states);
        let mut lowered: Vec<BindingId> = definitions
            .filter(|(binding, value)| {
                states
                    .get(binding)
                    .is_some_and(|state| state.is_lowered_to(value))
            })
            .map(|(binding, _)| binding)
            .collect();
        assert_eq!(
            lowered.len(),
            states.len(),
            "every global to take is lowered to the expression of its definition"
        );
        lowered.sort_unstable();
        LoweredGlobals(lowered)
    }

    pub fn restore_lowered<'m>(
        &mut self,
        lowered: &LoweredGlobals,
        definitions: impl Iterator<Item = (BindingId, &'m Rc<Expr>)>,
    ) {
        for (binding, value) in definitions.filter(|(binding, _)| lowered.contains(*binding)) {
            self.states
                .insert(binding, GlobalState::Lowered(Rc::clone(value)));
        }
    }
}

impl GlobalState {
    fn is_lowered_to(&self, value: &Rc<Expr>) -> bool {
        matches!(self, Self::Lowered(expr) if Rc::ptr_eq(expr, value))
    }
}

impl LoweredGlobals {
    fn contains(&self, binding: BindingId) -> bool {
        self.0.binary_search(&binding).is_ok()
    }
}

impl Expander<'_> {
    pub(super) fn lowered_global(
        &mut self,
        binding: BindingId,
        span: Span,
    ) -> Result<Rc<Expr>, Diagnostic> {
        self.decode_saved_global(binding);
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
            Some(failed @ (GlobalState::Failed | GlobalState::FailedDefinition(_))) => {
                self.tables.globals.states.insert(binding, failed);
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

    fn decode_saved_global(&mut self, binding: BindingId) {
        let states = &mut self.tables.globals.states;
        if !states.contains_key(&binding)
            && let Some(value) = self.saved_definitions.value_of(binding)
        {
            states.insert(binding, GlobalState::Lowered(value));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{BindingArena, BindingInfo, BindingKind, ExprKind, Leaf, Literal, ModuleId};
    use crate::source::SourceId;

    struct Definitions {
        bindings: BindingArena,
        values: Vec<(BindingId, Rc<Expr>)>,
    }

    impl Definitions {
        fn new() -> Self {
            Self {
                bindings: BindingArena::default(),
                values: Vec::new(),
            }
        }

        fn define(&mut self, name: &str) -> BindingId {
            let kind = BindingKind::Global(ModuleId::new(0));
            let binding = self.bindings.add(BindingInfo::new(name.into(), kind, None));
            let span = Span::new(SourceId::new(0), 0..0);
            let value = Expr::new(ExprKind::Leaf(Leaf::Literal(Literal::Bool(true))), span);
            self.values.push((binding, Rc::new(value)));
            binding
        }

        fn lower(&self, table: &mut GlobalTable, binding: BindingId) {
            table.states.insert(
                binding,
                GlobalState::Lowered(Rc::clone(self.value(binding))),
            );
        }

        fn value(&self, binding: BindingId) -> &Rc<Expr> {
            &self
                .values
                .iter()
                .find(|(defined, _)| *defined == binding)
                .expect("the binding is defined")
                .1
        }

        fn newest_first(&self) -> impl Iterator<Item = (BindingId, &Rc<Expr>)> {
            self.values
                .iter()
                .rev()
                .map(|(binding, value)| (*binding, value))
        }
    }

    #[test]
    fn a_restore_brings_back_only_the_lowered_globals() {
        let mut definitions = Definitions::new();
        let lowered = [definitions.define("fuel"), definitions.define("range")];
        let method = definitions.define("charge");
        let mut built = GlobalTable::default();
        lowered
            .iter()
            .for_each(|binding| definitions.lower(&mut built, *binding));
        let taken = built.take_lowered(definitions.newest_first());
        let mut restored = GlobalTable::default();
        restored.restore_lowered(&taken, definitions.newest_first());
        assert!(lowered.iter().all(|binding| {
            restored.states[binding].is_lowered_to(definitions.value(*binding))
        }));
        assert!(!restored.states.contains_key(&method));
    }

    #[test]
    fn a_take_leaves_no_state_behind() {
        let mut definitions = Definitions::new();
        let binding = definitions.define("fuel");
        let mut table = GlobalTable::default();
        definitions.lower(&mut table, binding);
        table.take_lowered(definitions.newest_first());
        assert!(table.states.is_empty());
    }

    #[test]
    #[should_panic(
        expected = "every global to take is lowered to the expression of its definition"
    )]
    fn a_global_lowered_to_another_expression_stops_the_take() {
        let mut definitions = Definitions::new();
        let binding = definitions.define("fuel");
        let mut table = GlobalTable::default();
        let copy = Expr::clone(definitions.value(binding));
        table
            .states
            .insert(binding, GlobalState::Lowered(Rc::new(copy)));
        table.take_lowered(definitions.newest_first());
    }
}
