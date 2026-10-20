use std::collections::HashMap;

use super::problems::OwnershipProblems;
use crate::core::{BindingArena, BindingId};
use crate::diagnostic::Diagnostic;
use crate::types::{
    Borrow, Type, TypedArgument, TypedBlock, TypedBodyItem, TypedDefinition, TypedExpr,
    TypedExprKind,
};

pub struct BorrowedResults;

impl BorrowedResults {
    pub fn check(definition: &TypedDefinition, bindings: &BindingArena) -> Option<Diagnostic> {
        let Type::Function(parameters, result) = &definition.value.ty else {
            return None;
        };
        let borrowed_parameters = parameters
            .iter()
            .filter(|parameter| parameter.is_reference())
            .count();
        let is_ambiguous = result.is_reference() && borrowed_parameters > 1;
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
        match &expr.kind {
            TypedExprKind::Call(callee, arguments) => self.call(callee, arguments),
            TypedExprKind::Fn(lambda) => self.block(&lambda.body),
            TypedExprKind::Block(block) => self.block(block),
            TypedExprKind::If(test, then, otherwise) => [test, then, otherwise]
                .into_iter()
                .for_each(|part| self.expr(part)),
            TypedExprKind::New(_, fields) => fields.iter().for_each(|(_, value)| self.expr(value)),
            TypedExprKind::Field(value, _) => self.expr(value),
            TypedExprKind::Literal(_)
            | TypedExprKind::Local(_)
            | TypedExprKind::Global(_)
            | TypedExprKind::Rust(_) => {}
        }
    }

    fn call(&mut self, callee: &TypedExpr, arguments: &[TypedArgument]) {
        self.expr(callee);
        let parameters = self.parameters(callee);
        for (position, argument) in arguments.iter().enumerate() {
            self.expr(&argument.value);
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
        if let TypedExprKind::Global(function) = callee.kind
            && let Some(parameters) = self.function_params.get(&function)
        {
            return parameters.clone();
        }
        match &callee.ty {
            Type::Function(parameters, _) => parameters.clone(),
            _ => Vec::new(),
        }
    }

    fn block(&mut self, block: &TypedBlock) {
        for item in &block.items {
            match item {
                TypedBodyItem::Let(_, value) | TypedBodyItem::Expr(value) => self.expr(value),
            }
        }
    }
}
