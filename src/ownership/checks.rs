use std::collections::HashMap;

use super::problems::OwnershipProblems;
use crate::core::BindingId;
use crate::diagnostic::Diagnostic;
use crate::types::{Borrow, Type, TypedArgument, TypedExpr, TypedExprKind, TypedLeaf};

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

    pub fn check(mut self, binding: BindingId, value: &TypedExpr) -> Vec<Diagnostic> {
        let current = self.function_params.get(&binding).cloned();
        match &value.kind {
            TypedExprKind::Fn(_) => value
                .children()
                .into_iter()
                .for_each(|child| self.value(child, current.as_deref())),
            _ => self.value(value, current.as_deref()),
        }
        self.diagnostics
    }

    fn value(&mut self, expr: &TypedExpr, current: Option<&[Type]>) {
        match &expr.kind {
            TypedExprKind::Call(callee, arguments) => self.check_arguments(callee, arguments),
            TypedExprKind::Recur(arguments) => self.check_recur(arguments, current),
            _ => {}
        }
        let child_current = match expr.kind {
            TypedExprKind::Fn(_) => None,
            _ => current,
        };
        for child in expr.children() {
            self.value(child, child_current);
        }
    }

    fn check_arguments(&mut self, callee: &TypedExpr, arguments: &[TypedArgument]) {
        let parameters = self.parameters(callee);
        self.check_with_parameters(arguments, &parameters);
    }

    fn check_recur(&mut self, arguments: &[TypedArgument], parameters: Option<&[Type]>) {
        if let Some(parameters) = parameters {
            self.check_with_parameters(arguments, parameters);
        }
    }

    fn check_with_parameters(&mut self, arguments: &[TypedArgument], parameters: &[Type]) {
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
            Type::Function(_, parameters, _) => parameters.clone(),
            _ => Vec::new(),
        }
    }
}
