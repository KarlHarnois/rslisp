use std::collections::{HashMap, HashSet};

use crate::core::BindingId;
use crate::types::{Type, TypedBinder, TypedExpr, TypedExprKind, TypedLambda, TypedProgram};

pub struct BorrowingLambdas<'p> {
    program: &'p TypedProgram,
    borrowed_parameters: HashMap<BindingId, HashSet<BindingId>>,
}

impl<'p> BorrowingLambdas<'p> {
    pub fn in_program(program: &'p TypedProgram) -> HashMap<BindingId, HashSet<BindingId>> {
        let mut found = Self {
            program,
            borrowed_parameters: HashMap::new(),
        };
        program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .flat_map(|definition| definition.value.let_bindings())
            .for_each(|(binder, value)| found.register(binder, value));
        found.borrowed_parameters
    }

    fn register(&mut self, binder: &TypedBinder, value: &TypedExpr) {
        if let Some(inner) = value.let_block_value() {
            return self.register(binder, inner);
        }
        let TypedExprKind::Fn(lambda) = &value.kind else {
            return;
        };
        if binder.annotated || self.program.escaping_locals.contains(&binder.binding) {
            return;
        }
        let borrowed = self.borrowed_parameters_of(lambda);
        self.borrowed_parameters.insert(binder.binding, borrowed);
    }

    fn borrowed_parameters_of(&self, lambda: &TypedLambda) -> HashSet<BindingId> {
        lambda
            .params
            .iter()
            .filter(|param| self.is_borrowed(param))
            .map(|param| param.binding)
            .collect()
    }

    fn is_borrowed(&self, param: &TypedBinder) -> bool {
        let stored = self
            .program
            .stored_function_parameters
            .contains(&param.binding);
        matches!(param.ty, Type::Function(..)) && !stored
    }
}
