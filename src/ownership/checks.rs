use std::collections::HashMap;

use super::problems::OwnershipProblems;
use crate::core::{BindingId, ReferenceKind};
use crate::diagnostic::Diagnostic;
use crate::source::Span;
use crate::types::{
    Borrow, Type, TypedArgument, TypedBinder, TypedBlock, TypedBodyItem, TypedControl,
    TypedDefinition, TypedExpr, TypedExprKind, TypedLeaf,
};

pub struct ReturnedBorrows;

impl ReturnedBorrows {
    pub fn check(definition: &TypedDefinition) -> Option<Diagnostic> {
        let TypedExprKind::Fn(lambda) = &definition.value.kind else {
            return None;
        };
        let Type::Function(_, _, result) = &definition.value.ty else {
            return None;
        };
        if !result.holds_reference() {
            return None;
        }
        let (borrowed, binding) =
            Self::borrowed_local(Self::tail_of(&lambda.body)?, &lambda.params)?;
        let defined = Self::local_span(&lambda.body, binding).unwrap_or(borrowed.span);
        Some(OwnershipProblems::local_borrow(borrowed.span, defined))
    }

    fn borrowed_local<'e>(
        expr: &'e TypedExpr,
        params: &[TypedBinder],
    ) -> Option<(&'e TypedExpr, BindingId)> {
        match &expr.kind {
            TypedExprKind::Borrow(ReferenceKind::Shared, inner)
            | TypedExprKind::Borrow(ReferenceKind::Mutable, inner) => {
                let binding = inner.place_root()?;
                params
                    .iter()
                    .all(|param| param.binding != binding)
                    .then_some((expr, binding))
            }
            TypedExprKind::Control(TypedControl::Block(block)) => {
                Self::tail_of(block).and_then(|tail| Self::borrowed_local(tail, params))
            }
            TypedExprKind::Control(TypedControl::If(_, then, otherwise)) => {
                Self::borrowed_local(then, params)
                    .or_else(|| Self::borrowed_local(otherwise, params))
            }
            TypedExprKind::Control(TypedControl::Match(matched)) => {
                matched.arms.iter().find_map(|arm| {
                    Self::tail_of(&arm.body).and_then(|tail| Self::borrowed_local(tail, params))
                })
            }
            _ => None,
        }
    }

    fn tail_of(block: &TypedBlock) -> Option<&TypedExpr> {
        match block.items.last()? {
            TypedBodyItem::Expr(expr) => Some(expr),
            TypedBodyItem::Let(..) => None,
        }
    }

    fn local_span(block: &TypedBlock, binding: BindingId) -> Option<Span> {
        block.items.iter().find_map(|item| match item {
            TypedBodyItem::Let(binder, _) if binder.binding == binding => Some(binder.span),
            _ => None,
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
