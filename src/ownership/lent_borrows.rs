use rustc_hash::FxHashMap;

use super::aliases::LetAliases;
use super::local_borrows::{LocalBorrows, ParameterOwner};
use super::problems::{LentParameter, OwnershipProblems};
use super::promoted_constants::PromotedConstants;
use crate::core::BindingId;
use crate::diagnostic::Diagnostic;
use crate::types::{
    TypedArgument, TypedBinder, TypedDefinition, TypedExpr, TypedExprKind, TypedLeaf, TypedProgram,
};

pub struct LentBorrows<'p> {
    program: &'p TypedProgram,
}

struct LendingClosure<'d> {
    origin: BindingId,
    parameters: Vec<Option<&'d TypedBinder>>,
}

type LocalCall<'e> = (BindingId, &'e [TypedArgument]);

impl<'p> LentBorrows<'p> {
    pub fn new(program: &'p TypedProgram) -> Self {
        Self { program }
    }

    pub fn check_definition(&self, definition: &TypedDefinition) -> Vec<Diagnostic> {
        let TypedExprKind::Fn(function) = &definition.value.kind else {
            return Vec::new();
        };
        let closures = Self::lending_closures(&definition.value);
        let constants = PromotedConstants::new(self.program);
        let borrows = LocalBorrows::dropped_in(function, ParameterOwner::Function, constants);
        Self::local_calls(&definition.value)
            .into_iter()
            .filter_map(|(called, arguments)| Some((called, closures.get(&called)?, arguments)))
            .flat_map(|(called, closure, arguments)| {
                self.lent_locals(&borrows, called, closure, arguments)
            })
            .collect()
    }

    fn lending_closures(value: &TypedExpr) -> FxHashMap<BindingId, LendingClosure<'_>> {
        let aliases = LetAliases::unannotated_in(value);
        value
            .let_bindings()
            .filter(|(binder, _)| !binder.annotated)
            .map(|(binder, bound)| {
                let closure = LendingClosure {
                    origin: aliases.origin(binder.binding),
                    parameters: Self::lent_parameters(aliases.resolve(bound)),
                };
                (binder.binding, closure)
            })
            .collect()
    }

    fn lent_parameters(value: &TypedExpr) -> Vec<Option<&TypedBinder>> {
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

    fn local_calls(expr: &TypedExpr) -> Vec<LocalCall<'_>> {
        let mut calls = Vec::new();
        Self::collect_local_calls(expr, &mut calls);
        calls
    }

    fn collect_local_calls<'e>(expr: &'e TypedExpr, calls: &mut Vec<LocalCall<'e>>) {
        calls.extend(Self::local_call(expr));
        expr.children()
            .into_iter()
            .for_each(|child| Self::collect_local_calls(child, calls));
    }

    fn local_call(expr: &TypedExpr) -> Option<LocalCall<'_>> {
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
        called: BindingId,
        closure: &LendingClosure,
        arguments: &[TypedArgument],
    ) -> Vec<Diagnostic> {
        let called_name = self.program.bindings.get(called).name();
        let closure_name = self.program.bindings.get(closure.origin).name();
        arguments
            .iter()
            .zip(&closure.parameters)
            .filter_map(|(argument, parameter)| {
                let lent = LentParameter {
                    span: (*parameter)?.span,
                    closure: closure_name,
                };
                let dropped = borrows.borrowed_by(argument)?;
                Some(OwnershipProblems::lent_local_borrow(
                    argument.value.span,
                    &dropped,
                    lent,
                    called_name,
                ))
            })
            .collect()
    }
}
