use rustc_hash::{FxHashMap, FxHashSet};

use super::aliases::{LetAlias, LetAliases};
use super::call_scopes::{CallScopes, ResultScope, ScopedCall};
use super::local_borrows::{LocalBorrows, ParameterOwner};
use super::local_functions::LocalFunctions;
use super::problems::{AliasLabel, DroppedValue, LentCall, LentParameter, OwnershipProblems};
use super::promoted_constants::PromotedConstants;
use super::subtree::Subtree;
use crate::core::BindingId;
use crate::diagnostic::Diagnostic;
use crate::source::Span;
use crate::types::{
    TypedArgument, TypedBinder, TypedDefinition, TypedExpr, TypedExprKind, TypedProgram,
};

type LentReport = fn(Span, &DroppedValue, LentParameter, &LentCall) -> Diagnostic;

pub struct LentBorrows<'p> {
    program: &'p TypedProgram,
}

struct LendingClosure<'d> {
    links: Vec<LetAlias<'d>>,
    parameters: Vec<Option<&'d TypedBinder>>,
    local_function: bool,
}

impl LendingClosure<'_> {
    fn origin(&self, called: BindingId) -> BindingId {
        self.links.last().map_or(called, |link| link.target)
    }

    fn result_outlives(&self, scope: &ResultScope, dropped: &DroppedValue) -> bool {
        !self.local_function || scope.outlives(dropped)
    }

    fn report(&self) -> LentReport {
        if self.local_function {
            OwnershipProblems::outlived_lent_borrow
        } else {
            OwnershipProblems::lent_local_borrow
        }
    }
}

struct DroppedValues<'e> {
    borrows: LocalBorrows<'e>,
    closure_parameters: FxHashSet<BindingId>,
}

impl<'e> DroppedValues<'e> {
    fn borrowed_by(&self, argument: &'e TypedArgument) -> Option<DroppedValue> {
        match self.borrows.borrowed_by(argument)? {
            DroppedValue::Local(span, binding) if self.closure_parameters.contains(&binding) => {
                Some(DroppedValue::ClosureParameter(span, binding))
            }
            dropped => Some(dropped),
        }
    }
}

impl<'p> LentBorrows<'p> {
    pub fn new(program: &'p TypedProgram) -> Self {
        Self { program }
    }

    pub fn check_definition(&self, definition: &TypedDefinition) -> Vec<Diagnostic> {
        let TypedExprKind::Fn(function) = &definition.value.kind else {
            return Vec::new();
        };
        let closures = self.lending_closures(&definition.value);
        let constants = PromotedConstants::new(self.program);
        let dropped_values = DroppedValues {
            borrows: LocalBorrows::dropped_in(function, ParameterOwner::Function, constants),
            closure_parameters: Self::closure_parameters(&definition.value),
        };
        let local_functions: FxHashSet<BindingId> = closures
            .iter()
            .filter(|(_, closure)| closure.local_function)
            .map(|(binding, _)| *binding)
            .collect();
        CallScopes::of(self.program, &local_functions, &definition.value)
            .into_iter()
            .filter_map(|call| Some((closures.get(&call.called)?, call)))
            .flat_map(|(closure, call)| self.lent_locals(&dropped_values, closure, &call))
            .collect()
    }

    fn lending_closures<'e>(
        &self,
        value: &'e TypedExpr,
    ) -> FxHashMap<BindingId, LendingClosure<'e>> {
        let aliases = LetAliases::unannotated_in(value);
        let function_lets = self.function_lets(value);
        aliases
            .entries()
            .map(|(binding, bound)| {
                let closure = aliases.resolve(bound);
                let mut lending = LendingClosure {
                    links: aliases.links(binding),
                    parameters: LocalFunctions::lent_parameters(closure),
                    local_function: false,
                };
                lending.local_function = function_lets.contains(&lending.origin(binding));
                (binding, lending)
            })
            .collect()
    }

    fn closure_parameters(value: &TypedExpr) -> FxHashSet<BindingId> {
        Subtree::of(value)
            .into_iter()
            .skip(1)
            .filter_map(|expr| match &expr.kind {
                TypedExprKind::Fn(lambda) => Some(lambda),
                _ => None,
            })
            .flat_map(|lambda| lambda.params.iter().map(|param| param.binding))
            .collect()
    }

    fn function_lets(&self, value: &TypedExpr) -> FxHashSet<BindingId> {
        let local_functions = LocalFunctions::new(self.program);
        value
            .let_bindings()
            .filter(|(binder, closure)| local_functions.emits_as_function(binder, closure))
            .map(|(binder, _)| binder.binding)
            .collect()
    }

    fn lent_locals(
        &self,
        dropped_values: &DroppedValues,
        closure: &LendingClosure,
        call: &ScopedCall,
    ) -> Vec<Diagnostic> {
        let lent_call = LentCall {
            called: self.name(call.called),
            aliases: closure
                .links
                .iter()
                .map(|link| self.alias_label(link))
                .collect(),
        };
        let closure_name = self.name(closure.origin(call.called));
        call.arguments
            .iter()
            .zip(&closure.parameters)
            .filter_map(|(argument, parameter)| {
                let lent = LentParameter {
                    span: (*parameter)?.span,
                    closure: closure_name,
                };
                let dropped = dropped_values
                    .borrowed_by(argument)
                    .filter(|dropped| closure.result_outlives(&call.scope, dropped))?;
                Some(closure.report()(
                    argument.value.span,
                    &dropped,
                    lent,
                    &lent_call,
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
