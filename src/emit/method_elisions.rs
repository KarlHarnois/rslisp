use std::cell::RefCell;
use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::plan::EmitPlan;
use super::result_borrows::ResultBorrows;
use super::signature_borrows::SignatureBorrows;
use crate::core::BindingId;
use crate::types::{
    ReceiverKind, Type, TypedExprKind, TypedLambda, TypedProgram, TypedTraitMethod,
};

type TraitMethodKey = (BindingId, String);

#[derive(Default)]
pub struct MethodLifetimes {
    elided: FxHashSet<usize>,
    shared: FxHashSet<usize>,
}

impl MethodLifetimes {
    fn sharing(called_with_local: FxHashSet<usize>, flowing: FxHashSet<usize>) -> Self {
        Self {
            elided: called_with_local,
            shared: flowing,
        }
    }

    fn eliding(called_with_local: FxHashSet<usize>, flowing: FxHashSet<usize>) -> Self {
        Self {
            elided: called_with_local.union(&flowing).copied().collect(),
            shared: FxHashSet::default(),
        }
    }

    pub fn elided(&self) -> &FxHashSet<usize> {
        &self.elided
    }

    pub fn shares_lifetime(&self) -> bool {
        !self.shared.is_empty()
    }

    pub fn takes_shared_lifetime(&self, position: usize) -> bool {
        self.shared.contains(&position)
    }

    pub fn keeps_own_lifetime(&self, position: usize) -> bool {
        self.shares_lifetime() && !self.takes_shared_lifetime(position)
    }
}

pub struct TraitMethodElisions {
    implemented_traits: FxHashMap<BindingId, BindingId>,
    lifetimes: RefCell<FxHashMap<TraitMethodKey, Rc<MethodLifetimes>>>,
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
            lifetimes: RefCell::default(),
        }
    }

    fn implemented_trait(&self, method: BindingId) -> Option<BindingId> {
        self.implemented_traits.get(&method).copied()
    }

    fn cached(&self, key: &TraitMethodKey) -> Option<Rc<MethodLifetimes>> {
        self.lifetimes.borrow().get(key).cloned()
    }

    fn remember(&self, key: TraitMethodKey, lifetimes: Rc<MethodLifetimes>) {
        self.lifetimes.borrow_mut().insert(key, lifetimes);
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

    pub fn lifetimes(&self, method: BindingId) -> Rc<MethodLifetimes> {
        let Some(trait_binding) = self.trait_of(method) else {
            return Rc::default();
        };
        let name = self.program.bindings.get(method).name();
        let hides_argument = !self
            .arguments_behind_receiver(trait_binding, name)
            .is_empty();
        if !hides_argument
            && self
                .lambda(method)
                .is_some_and(|lambda| !self.takes_borrowing_function(lambda))
        {
            return Rc::default();
        }
        self.cached_lifetimes(trait_binding, name)
    }

    fn cached_lifetimes(&self, trait_binding: BindingId, name: &str) -> Rc<MethodLifetimes> {
        let key = (trait_binding, name.to_string());
        if let Some(cached) = self.trait_method_elisions.cached(&key) {
            return cached;
        }
        let lifetimes = Rc::new(self.lifetimes_in_bodies(trait_binding, name));
        self.trait_method_elisions
            .remember(key, Rc::clone(&lifetimes));
        lifetimes
    }

    fn lifetimes_in_bodies(&self, trait_binding: BindingId, name: &str) -> MethodLifetimes {
        let behind_receiver = self.arguments_behind_receiver(trait_binding, name);
        let bodies: Vec<&TypedLambda> = self
            .bodies(trait_binding, name)
            .into_iter()
            .filter(|lambda| !behind_receiver.is_empty() || self.takes_borrowing_function(lambda))
            .collect();
        let (called_with_local, flowing) = self.parameter_positions(&bodies);
        let receiver = self.receiver_of(trait_binding, name);
        if !flowing.is_disjoint(&behind_receiver)
            || bodies
                .iter()
                .any(|lambda| self.flows_past_elision(lambda, &flowing, receiver))
        {
            MethodLifetimes::sharing(called_with_local, flowing)
        } else {
            MethodLifetimes::eliding(called_with_local, flowing)
        }
    }

    fn parameter_positions(&self, bodies: &[&TypedLambda]) -> (FxHashSet<usize>, FxHashSet<usize>) {
        let mut called_with_local = FxHashSet::default();
        let mut flowing = FxHashSet::default();
        for lambda in bodies {
            let borrows = self.result_borrows.analyse(lambda);
            called_with_local.extend(Self::positions(lambda, &borrows.higher_ranked));
            flowing.extend(Self::positions(lambda, &borrows.flows));
        }
        flowing.retain(|position| !called_with_local.contains(position));
        (called_with_local, flowing)
    }

    fn positions(lambda: &TypedLambda, bindings: &FxHashSet<BindingId>) -> Vec<usize> {
        lambda
            .params
            .iter()
            .enumerate()
            .filter(|(_, param)| bindings.contains(&param.binding))
            .map(|(position, _)| position)
            .collect()
    }

    fn flows_past_elision(
        &self,
        lambda: &TypedLambda,
        flowing: &FxHashSet<usize>,
        receiver: Option<ReceiverKind>,
    ) -> bool {
        let borrows = SignatureBorrows::new(self.program);
        flowing.iter().any(|position| {
            let ty = &lambda.params[*position].ty;
            match receiver {
                Some(_) => borrows.returns_borrow_from_no_argument(ty),
                None => borrows.hides_borrow_from_elision(ty),
            }
        })
    }

    fn bodies(&self, trait_binding: BindingId, name: &str) -> Vec<&'a TypedLambda> {
        self.program
            .trait_method_bodies(trait_binding, name)
            .into_iter()
            .filter_map(|binding| self.lambda(binding))
            .collect()
    }

    fn arguments_behind_receiver(&self, trait_binding: BindingId, name: &str) -> FxHashSet<usize> {
        let Some(declared) = self.declared(trait_binding, name) else {
            return FxHashSet::default();
        };
        let borrowed_receiver = matches!(
            declared.receiver,
            Some(ReceiverKind::Shared | ReceiverKind::Mutable)
        );
        match self
            .plan
            .definition(declared.declaration())
            .map(|definition| &definition.scheme.body)
        {
            Some(Type::Function(_, parameters, _)) if borrowed_receiver => {
                Self::borrowed_arguments(self.program, parameters)
            }
            _ => FxHashSet::default(),
        }
    }

    fn borrowed_arguments(program: &TypedProgram, parameters: &[Type]) -> FxHashSet<usize> {
        parameters
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(_, parameter)| program.type_holds_borrow(parameter))
            .map(|(position, _)| position)
            .collect()
    }

    fn receiver_of(&self, trait_binding: BindingId, name: &str) -> Option<ReceiverKind> {
        self.declared(trait_binding, name)
            .and_then(|declared| declared.receiver)
    }

    fn declared(&self, trait_binding: BindingId, name: &str) -> Option<&'a TypedTraitMethod> {
        self.program.declared_trait_method(trait_binding, name)
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

    fn takes_borrowing_function(&self, lambda: &TypedLambda) -> bool {
        lambda
            .params
            .iter()
            .any(|param| self.program.is_function_returning_borrow(&param.ty))
    }
}
