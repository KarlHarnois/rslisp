use rustc_hash::FxHashMap;

use super::aliases::{LetAlias, LetAliases};
use super::local_borrows::{LocalBorrows, ParameterOwner};
use super::problems::{AliasLabel, LentCall, LentParameter, OwnershipProblems};
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
    links: Vec<LetAlias<'d>>,
    parameters: Vec<Option<&'d TypedBinder>>,
}

impl LendingClosure<'_> {
    fn origin(&self, called: BindingId) -> BindingId {
        self.links.last().map_or(called, |link| link.target)
    }
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
        aliases
            .entries()
            .map(|(binding, bound)| {
                let closure = LendingClosure {
                    links: aliases.links(binding),
                    parameters: Self::lent_parameters(aliases.resolve(bound)),
                };
                (binding, closure)
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
        let call = LentCall {
            called: self.name(called),
            aliases: closure
                .links
                .iter()
                .map(|link| self.alias_label(link))
                .collect(),
        };
        let closure_name = self.name(closure.origin(called));
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
                    &call,
                ))
            })
            .collect()
    }

    fn alias_label(&self, link: &LetAlias) -> AliasLabel<'p> {
        AliasLabel {
            span: link.bound.span,
            alias: self.name(link.alias),
            target: self.name(link.target),
        }
    }

    fn name(&self, binding: BindingId) -> &'p str {
        self.program.bindings.get(binding).name()
    }
}
