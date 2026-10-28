use std::collections::HashSet;

use super::plan::EmitPlan;
use super::result_borrows::ResultBorrows;
use crate::core::BindingId;
use crate::types::{TypedExprKind, TypedLambda, TypedProgram};

pub struct MethodElisions<'r, 'a> {
    program: &'a TypedProgram,
    plan: &'a EmitPlan<'a>,
    result_borrows: &'r ResultBorrows<'a>,
}

impl<'r, 'a> MethodElisions<'r, 'a> {
    pub fn new(
        program: &'a TypedProgram,
        plan: &'a EmitPlan<'a>,
        result_borrows: &'r ResultBorrows<'a>,
    ) -> Self {
        Self {
            program,
            plan,
            result_borrows,
        }
    }

    pub fn elided_positions(&self, method: BindingId) -> HashSet<usize> {
        if self
            .lambda(method)
            .is_some_and(|lambda| !self.takes_borrowing_function(lambda))
        {
            return HashSet::new();
        }
        self.bodies(method)
            .into_iter()
            .flat_map(|lambda| self.higher_ranked_positions(lambda))
            .collect()
    }

    fn bodies(&self, method: BindingId) -> Vec<&'a TypedLambda> {
        let Some(trait_binding) = self.trait_of(method) else {
            return Vec::new();
        };
        let name = self.program.bindings.get(method).name();
        self.program
            .trait_impls
            .iter()
            .filter(|implementation| implementation.trait_binding == trait_binding)
            .flat_map(|implementation| &implementation.methods)
            .map(|implemented| implemented.binding)
            .filter(|binding| self.program.bindings.get(*binding).name() == name)
            .chain(self.default_of(trait_binding, name))
            .filter_map(|binding| self.lambda(binding))
            .collect()
    }

    fn default_of(&self, trait_binding: BindingId, name: &str) -> Option<BindingId> {
        self.program
            .trait_definitions
            .iter()
            .filter(|definition| definition.binding == trait_binding)
            .flat_map(|definition| &definition.methods)
            .find(|declared| self.program.bindings.get(declared.binding).name() == name)
            .and_then(|declared| declared.default)
    }

    fn trait_of(&self, method: BindingId) -> Option<BindingId> {
        if let Some(info) = self.program.trait_methods.get(&method) {
            return Some(info.trait_binding);
        }
        self.program
            .trait_impls
            .iter()
            .find(|implementation| {
                implementation
                    .methods
                    .iter()
                    .any(|implemented| implemented.binding == method)
            })
            .map(|implementation| implementation.trait_binding)
    }

    fn lambda(&self, binding: BindingId) -> Option<&'a TypedLambda> {
        match &self.plan.definition(binding)?.value.kind {
            TypedExprKind::Fn(lambda) => Some(lambda),
            _ => None,
        }
    }

    fn higher_ranked_positions(&self, lambda: &TypedLambda) -> Vec<usize> {
        if !self.takes_borrowing_function(lambda) {
            return Vec::new();
        }
        let borrows = self.result_borrows.analyse(lambda);
        lambda
            .params
            .iter()
            .enumerate()
            .filter(|(_, param)| {
                borrows.higher_ranked.contains(&param.binding)
                    || borrows.flows.contains(&param.binding)
            })
            .map(|(position, _)| position)
            .collect()
    }

    fn takes_borrowing_function(&self, lambda: &TypedLambda) -> bool {
        lambda
            .params
            .iter()
            .any(|param| self.program.is_borrowing_function(&param.ty))
    }
}
