use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::plan::EmitPlan;
use super::result_borrows::ResultBorrows;
use crate::core::BindingId;
use crate::types::{TypedExprKind, TypedLambda, TypedProgram};

type TraitMethodKey = (BindingId, String);

pub struct TraitMethodElisions {
    implemented_traits: HashMap<BindingId, BindingId>,
    positions: RefCell<HashMap<TraitMethodKey, Rc<HashSet<usize>>>>,
}

impl TraitMethodElisions {
    pub fn new(program: &TypedProgram) -> Self {
        let implemented_traits = program
            .trait_impls
            .iter()
            .flat_map(|implementation| {
                implementation
                    .methods
                    .iter()
                    .map(|implemented| (implemented.binding, implementation.trait_binding))
            })
            .collect();
        Self {
            implemented_traits,
            positions: RefCell::default(),
        }
    }

    fn implemented_trait(&self, method: BindingId) -> Option<BindingId> {
        self.implemented_traits.get(&method).copied()
    }

    fn cached(&self, key: &TraitMethodKey) -> Option<Rc<HashSet<usize>>> {
        self.positions.borrow().get(key).cloned()
    }

    fn remember(&self, key: TraitMethodKey, positions: Rc<HashSet<usize>>) {
        self.positions.borrow_mut().insert(key, positions);
    }
}

pub struct MethodElisions<'r, 'a> {
    program: &'a TypedProgram,
    plan: &'a EmitPlan<'a>,
    result_borrows: &'r ResultBorrows<'a>,
    trait_method_elisions: &'r TraitMethodElisions,
}

impl<'r, 'a> MethodElisions<'r, 'a> {
    pub fn new(
        program: &'a TypedProgram,
        plan: &'a EmitPlan<'a>,
        result_borrows: &'r ResultBorrows<'a>,
        trait_method_elisions: &'r TraitMethodElisions,
    ) -> Self {
        Self {
            program,
            plan,
            result_borrows,
            trait_method_elisions,
        }
    }

    pub fn elided_positions(&self, method: BindingId) -> Rc<HashSet<usize>> {
        if self
            .lambda(method)
            .is_some_and(|lambda| !self.takes_borrowing_function(lambda))
        {
            return Rc::default();
        }
        let Some(trait_binding) = self.trait_of(method) else {
            return Rc::default();
        };
        let name = self.program.bindings.get(method).name();
        self.cached_positions(trait_binding, name)
    }

    fn cached_positions(&self, trait_binding: BindingId, name: &str) -> Rc<HashSet<usize>> {
        let key = (trait_binding, name.to_string());
        if let Some(cached) = self.trait_method_elisions.cached(&key) {
            return cached;
        }
        let positions = Rc::new(self.positions_in_bodies(trait_binding, name));
        self.trait_method_elisions
            .remember(key, Rc::clone(&positions));
        positions
    }

    fn positions_in_bodies(&self, trait_binding: BindingId, name: &str) -> HashSet<usize> {
        self.bodies(trait_binding, name)
            .into_iter()
            .flat_map(|lambda| self.higher_ranked_positions(lambda))
            .collect()
    }

    fn bodies(&self, trait_binding: BindingId, name: &str) -> Vec<&'a TypedLambda> {
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
        self.program
            .trait_methods
            .get(&method)
            .map(|info| info.trait_binding)
            .or_else(|| self.trait_method_elisions.implemented_trait(method))
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
