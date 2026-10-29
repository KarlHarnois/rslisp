use std::collections::{HashMap, HashSet};

use super::function_borrow::FunctionBorrow;
use crate::core::BindingId;
use crate::types::{
    Type, TypedArgument, TypedBinder, TypedExpr, TypedExprKind, TypedLambda, TypedLeaf,
    TypedProgram,
};

pub struct LocalClosures<'a> {
    program: &'a TypedProgram,
    argument_borrows: HashMap<BindingId, Vec<Option<FunctionBorrow>>>,
    borrowed_parameters: HashMap<BindingId, FunctionBorrow>,
    mutably_borrowed_arguments: HashSet<BindingId>,
}

impl<'a> LocalClosures<'a> {
    pub fn new(program: &'a TypedProgram) -> Self {
        Self {
            program,
            argument_borrows: HashMap::new(),
            borrowed_parameters: HashMap::new(),
            mutably_borrowed_arguments: HashSet::new(),
        }
    }

    pub fn of(program: &'a TypedProgram, value: &TypedExpr) -> Self {
        let mut found = Self::new(program);
        value
            .let_bindings()
            .for_each(|(binder, value)| found.register(binder, value));
        if !found.argument_borrows.is_empty() {
            found.expr(value);
        }
        found
    }

    pub fn borrows_arguments_of(&self, closure: BindingId) -> bool {
        self.argument_borrows.contains_key(&closure)
    }

    pub fn parameter_borrow(&self, parameter: BindingId) -> Option<FunctionBorrow> {
        self.borrowed_parameters.get(&parameter).copied()
    }

    pub fn argument_borrow(
        &self,
        callee: &TypedExpr,
        position: usize,
        argument: &TypedExpr,
    ) -> Option<FunctionBorrow> {
        let TypedExprKind::Leaf(TypedLeaf::Local(closure)) = callee.kind else {
            return None;
        };
        if self.is_borrowed_parameter(argument) {
            return None;
        }
        self.argument_borrows
            .get(&closure)?
            .get(position)
            .copied()
            .flatten()
    }

    pub fn mutably_borrowed_arguments(&self) -> impl Iterator<Item = BindingId> + '_ {
        self.mutably_borrowed_arguments.iter().copied()
    }

    fn is_borrowed_parameter(&self, argument: &TypedExpr) -> bool {
        matches!(argument.kind, TypedExprKind::Leaf(TypedLeaf::Local(local))
            if self.borrowed_parameters.contains_key(&local))
    }

    fn expr(&mut self, expr: &TypedExpr) {
        match &expr.kind {
            TypedExprKind::Call(callee, arguments) => self.call(callee, arguments),
            _ => expr
                .children()
                .into_iter()
                .for_each(|child| self.expr(child)),
        }
    }

    fn register(&mut self, binder: &TypedBinder, value: &TypedExpr) {
        let Some(lambda) = value.lambda_value() else {
            return;
        };
        if let Some(borrowed) = self.program.borrowing_lambdas.get(&binder.binding) {
            self.register_closure(binder.binding, lambda, borrowed);
        }
    }

    fn register_closure(
        &mut self,
        closure: BindingId,
        lambda: &TypedLambda,
        borrowed: &HashSet<BindingId>,
    ) {
        let borrows: Vec<Option<FunctionBorrow>> = lambda
            .params
            .iter()
            .map(|param| Self::function_borrow(param, borrowed))
            .collect();
        let parameter_borrows = lambda
            .params
            .iter()
            .zip(&borrows)
            .filter_map(|(param, borrow)| borrow.map(|borrow| (param.binding, borrow)));
        self.borrowed_parameters.extend(parameter_borrows);
        self.argument_borrows.insert(closure, borrows);
    }

    fn function_borrow(
        param: &TypedBinder,
        borrowed: &HashSet<BindingId>,
    ) -> Option<FunctionBorrow> {
        let Type::Function(kind, _, _) = param.ty else {
            return None;
        };
        borrowed
            .contains(&param.binding)
            .then(|| FunctionBorrow::of(kind))
    }

    fn call(&mut self, callee: &TypedExpr, arguments: &[TypedArgument]) {
        for (position, argument) in arguments.iter().enumerate() {
            self.note_mutable_borrow(callee, position, &argument.value);
        }
        self.expr(callee);
        arguments
            .iter()
            .for_each(|argument| self.expr(&argument.value));
    }

    fn note_mutable_borrow(&mut self, callee: &TypedExpr, position: usize, argument: &TypedExpr) {
        let TypedExprKind::Leaf(TypedLeaf::Local(local)) = argument.kind else {
            return;
        };
        if let Some(FunctionBorrow::Mutable) = self.argument_borrow(callee, position, argument) {
            self.mutably_borrowed_arguments.insert(local);
        }
    }
}
