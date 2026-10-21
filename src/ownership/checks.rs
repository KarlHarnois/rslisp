use std::collections::HashMap;

use super::problems::OwnershipProblems;
use crate::core::{BindingArena, BindingId};
use crate::diagnostic::Diagnostic;
use crate::types::{
    Borrow, Type, TypedArgument, TypedDefinition, TypedExpr, TypedExprKind, TypedLeaf,
};

pub struct BorrowedResults;

impl BorrowedResults {
    pub fn check(definition: &TypedDefinition, bindings: &BindingArena) -> Option<Diagnostic> {
        let Type::Function(parameters, result) = &definition.value.ty else {
            return None;
        };
        let borrowed_parameters = parameters
            .iter()
            .filter(|parameter| parameter.holds_reference())
            .count();
        let is_ambiguous = result.holds_reference() && borrowed_parameters > 1;
        is_ambiguous.then(|| {
            OwnershipProblems::ambiguous_borrow(
                bindings.get(definition.binding).name(),
                definition.span,
            )
        })
    }
}

pub struct OwnershipNeeds<'a> {
    function_params: &'a HashMap<BindingId, Vec<Type>>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> OwnershipNeeds<'a> {
    pub fn new(function_params: &'a HashMap<BindingId, Vec<Type>>) -> Self {
        Self {
            function_params,
            diagnostics: Vec::new(),
        }
    }

    pub fn check(mut self, value: &TypedExpr) -> Vec<Diagnostic> {
        self.expr(value);
        self.diagnostics
    }

    fn expr(&mut self, expr: &TypedExpr) {
        if let TypedExprKind::Call(callee, arguments) = &expr.kind {
            self.check_arguments(callee, arguments);
        }
        for child in expr.children() {
            self.expr(child);
        }
    }

    fn check_arguments(&mut self, callee: &TypedExpr, arguments: &[TypedArgument]) {
        let parameters = self.parameters(callee);
        for (position, argument) in arguments.iter().enumerate() {
            let owns = parameters
                .get(position)
                .is_some_and(|parameter| !parameter.is_reference());
            if argument.borrow == Borrow::Passthrough && owns {
                self.diagnostics
                    .push(OwnershipProblems::needs_ownership(argument.value.span));
            }
        }
    }

    fn parameters(&self, callee: &TypedExpr) -> Vec<Type> {
        if let TypedExprKind::Leaf(TypedLeaf::Global(function)) = callee.kind
            && let Some(parameters) = self.function_params.get(&function)
        {
            return parameters.clone();
        }
        match &callee.ty {
            Type::Function(parameters, _) => parameters.clone(),
            _ => Vec::new(),
        }
    }
}
