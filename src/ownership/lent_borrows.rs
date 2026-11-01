use std::collections::HashMap;

use super::local_borrows::{LocalBorrows, ParameterOwner};
use super::problems::OwnershipProblems;
use crate::core::BindingId;
use crate::diagnostic::Diagnostic;
use crate::types::{
    TypedArgument, TypedBinder, TypedDefinition, TypedExpr, TypedExprKind, TypedLeaf, TypedProgram,
};

pub struct LentBorrows<'p> {
    program: &'p TypedProgram,
}

type LentParameters<'d> = Vec<Option<&'d TypedBinder>>;

impl<'p> LentBorrows<'p> {
    pub fn new(program: &'p TypedProgram) -> Self {
        Self { program }
    }

    pub fn check_definition(&self, definition: &TypedDefinition) -> Vec<Diagnostic> {
        let TypedExprKind::Fn(function) = &definition.value.kind else {
            return Vec::new();
        };
        let closures = Self::lending_closures(&definition.value);
        let borrows = LocalBorrows::dropped_in(function, ParameterOwner::Function);
        Self::local_calls(&definition.value)
            .into_iter()
            .filter_map(|(closure, arguments)| Some((closure, closures.get(&closure)?, arguments)))
            .flat_map(|(closure, lent, arguments)| {
                self.lent_locals(&borrows, closure, lent, arguments)
            })
            .collect()
    }

    fn lending_closures(value: &TypedExpr) -> HashMap<BindingId, LentParameters<'_>> {
        value
            .let_bindings()
            .filter(|(binder, _)| !binder.annotated)
            .map(|(binder, value)| (binder.binding, Self::lent_parameters(value)))
            .collect()
    }

    fn lent_parameters(value: &TypedExpr) -> LentParameters<'_> {
        let TypedExprKind::Fn(lambda) = &value.kind else {
            return Vec::new();
        };
        let Some(tail) = LocalBorrows::tail_of(&lambda.body) else {
            return Vec::new();
        };
        lambda
            .params
            .iter()
            .map(|param| {
                LocalBorrows::lent_by(lambda, param.binding)
                    .returned(tail)
                    .map(|_| param)
            })
            .collect()
    }

    fn local_calls(expr: &TypedExpr) -> Vec<(BindingId, &[TypedArgument])> {
        let nested = expr.children().into_iter().flat_map(Self::local_calls);
        Self::local_call(expr).into_iter().chain(nested).collect()
    }

    fn local_call(expr: &TypedExpr) -> Option<(BindingId, &[TypedArgument])> {
        let TypedExprKind::Call(callee, arguments) = &expr.kind else {
            return None;
        };
        let TypedExprKind::Leaf(TypedLeaf::Local(closure)) = callee.kind else {
            return None;
        };
        Some((closure, arguments))
    }

    fn lent_locals(
        &self,
        borrows: &LocalBorrows,
        closure: BindingId,
        lent: &LentParameters,
        arguments: &[TypedArgument],
    ) -> Vec<Diagnostic> {
        let name = self.program.bindings.get(closure).name();
        arguments
            .iter()
            .zip(lent)
            .filter_map(|(argument, parameter)| {
                let parameter = (*parameter)?;
                let dropped = borrows.borrowed_by(argument)?;
                Some(OwnershipProblems::lent_local_borrow(
                    argument.value.span,
                    &dropped,
                    parameter.span,
                    name,
                ))
            })
            .collect()
    }
}
