use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::problems::{FunctionStorage, OwnershipProblems};
use super::slots::DeclaredSlots;
use crate::core::{BindingId, FieldLabel};
use crate::diagnostic::Diagnostic;
use crate::source::Span;
use crate::types::{
    Type, TypedArgument, TypedExpr, TypedExprKind, TypedLeaf, TypedProgram, TypedStructExpr,
};

pub struct StoredClosures<'p> {
    program: &'p TypedProgram,
    slots: FunctionSlots<'p>,
    storing: StoringParameters,
}

impl<'p> StoredClosures<'p> {
    pub fn check(program: &mut TypedProgram) -> Vec<Diagnostic> {
        let (diagnostics, stored) = {
            let program = &*program;
            let slots = FunctionSlots::new(program);
            let storing = StoringParameters::collect(program, &slots);
            let closures = StoredClosures {
                program,
                slots,
                storing,
            };
            (closures.diagnostics(), closures.stored_parameters())
        };
        program.stored_function_parameters = stored;
        diagnostics
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        self.program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .flat_map(|definition| Subtree::of(&definition.value))
            .flat_map(|expr| self.capturing_stores(expr))
            .collect()
    }

    fn stored_parameters(&self) -> HashSet<BindingId> {
        self.program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .filter_map(|definition| match &definition.value.kind {
                TypedExprKind::Fn(lambda) => Some((definition.binding, lambda)),
                _ => None,
            })
            .flat_map(|(function, lambda)| {
                lambda
                    .params
                    .iter()
                    .enumerate()
                    .filter_map(move |(position, param)| {
                        self.storing
                            .stores(function, position)
                            .then_some(param.binding)
                    })
            })
            .collect()
    }

    fn capturing_stores(&self, expr: &TypedExpr) -> Vec<Diagnostic> {
        Store::all_in(expr, &self.slots, &self.storing)
            .into_iter()
            .filter_map(|store| self.capturing_store(store))
            .collect()
    }

    fn capturing_store(&self, store: Store) -> Option<Diagnostic> {
        if !matches!(store.value.kind, TypedExprKind::Fn(_)) {
            return None;
        }
        let captured = Subtree::first_capture(store.value)?;
        Some(OwnershipProblems::capturing_function_field(
            store.value.span,
            self.program.bindings.get(captured).name(),
            &store.slot.storage,
            store.slot.declared,
        ))
    }
}

#[derive(Clone)]
struct FunctionSlot {
    storage: Rc<FunctionStorage>,
    declared: Span,
}

struct FunctionSlots<'p> {
    declared: DeclaredSlots<'p>,
}

impl<'p> FunctionSlots<'p> {
    fn new(program: &'p TypedProgram) -> Self {
        Self {
            declared: DeclaredSlots::new(program),
        }
    }

    fn field(&self, owner: BindingId, label: &FieldLabel) -> Option<FunctionSlot> {
        let slot = self.declared.field(owner, label)?;
        if !Self::is_function(slot.declared) {
            return None;
        }
        Some(FunctionSlot {
            storage: Rc::new(FunctionStorage::Field(Rc::clone(slot.name))),
            declared: slot.span,
        })
    }

    fn payload(&self, variant: BindingId, position: usize) -> Option<FunctionSlot> {
        let slot = self.declared.payload(variant, position)?;
        if !Self::is_function(slot.declared) {
            return None;
        }
        Some(FunctionSlot {
            storage: Rc::new(FunctionStorage::Payload(Rc::clone(slot.name))),
            declared: slot.span,
        })
    }

    fn is_function(declared: &Type) -> bool {
        matches!(declared, Type::Function(..))
    }
}

struct Store<'e> {
    value: &'e TypedExpr,
    slot: FunctionSlot,
}

impl<'e> Store<'e> {
    fn all_in(
        expr: &'e TypedExpr,
        slots: &FunctionSlots,
        storing: &StoringParameters,
    ) -> Vec<Self> {
        match &expr.kind {
            TypedExprKind::Struct(
                TypedStructExpr::New(owner, fields) | TypedStructExpr::Update(owner, _, fields),
            ) => Self::fields(*owner, fields, slots),
            TypedExprKind::Variant(variant, payloads) => Self::payloads(*variant, payloads, slots),
            TypedExprKind::Call(callee, arguments) => Self::arguments(callee, arguments, storing),
            _ => Vec::new(),
        }
    }

    fn fields(
        owner: BindingId,
        fields: &'e [(FieldLabel, TypedExpr)],
        slots: &FunctionSlots,
    ) -> Vec<Self> {
        fields
            .iter()
            .filter_map(|(label, value)| Some(Self::new(value, slots.field(owner, label)?)))
            .collect()
    }

    fn payloads(variant: BindingId, payloads: &'e [TypedExpr], slots: &FunctionSlots) -> Vec<Self> {
        payloads
            .iter()
            .enumerate()
            .filter_map(|(position, value)| {
                Some(Self::new(value, slots.payload(variant, position)?))
            })
            .collect()
    }

    fn arguments(
        callee: &TypedExpr,
        arguments: &'e [TypedArgument],
        storing: &StoringParameters,
    ) -> Vec<Self> {
        let TypedExprKind::Leaf(TypedLeaf::Global(function)) = callee.kind else {
            return Vec::new();
        };
        arguments
            .iter()
            .enumerate()
            .filter_map(|(position, argument)| {
                Some(Self::new(
                    &argument.value,
                    storing.slot(function, position)?,
                ))
            })
            .collect()
    }

    fn new(value: &'e TypedExpr, slot: FunctionSlot) -> Self {
        Self { value, slot }
    }
}

#[derive(Default)]
struct StoringParameters {
    slots: HashMap<(BindingId, usize), FunctionSlot>,
}

impl StoringParameters {
    fn collect(program: &TypedProgram, slots: &FunctionSlots) -> Self {
        let mut storing = Self::default();
        while storing.add_stores(program, slots) {}
        storing
    }

    fn slot(&self, function: BindingId, position: usize) -> Option<FunctionSlot> {
        self.slots.get(&(function, position)).cloned()
    }

    fn stores(&self, function: BindingId, position: usize) -> bool {
        self.slots.contains_key(&(function, position))
    }

    fn add_stores(&mut self, program: &TypedProgram, slots: &FunctionSlots) -> bool {
        let found: Vec<((BindingId, usize), FunctionSlot)> = program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .flat_map(|definition| {
                self.parameter_stores(definition.binding, &definition.value, slots)
            })
            .filter(|(key, _)| !self.slots.contains_key(key))
            .collect();
        let added = !found.is_empty();
        self.slots.extend(found);
        added
    }

    fn parameter_stores(
        &self,
        function: BindingId,
        value: &TypedExpr,
        slots: &FunctionSlots,
    ) -> Vec<((BindingId, usize), FunctionSlot)> {
        let TypedExprKind::Fn(lambda) = &value.kind else {
            return Vec::new();
        };
        let positions: HashMap<BindingId, usize> = lambda
            .params
            .iter()
            .enumerate()
            .map(|(position, param)| (param.binding, position))
            .collect();
        Subtree::of(value)
            .into_iter()
            .flat_map(|expr| Store::all_in(expr, slots, self))
            .filter_map(|store| {
                let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = store.value.kind else {
                    return None;
                };
                Some(((function, *positions.get(&binding)?), store.slot))
            })
            .collect()
    }
}

struct Subtree;

impl Subtree {
    fn of(root: &TypedExpr) -> Vec<&TypedExpr> {
        let mut expressions = vec![root];
        let mut next = 0;
        while let Some(expr) = expressions.get(next).copied() {
            expressions.extend(expr.children());
            next += 1;
        }
        expressions
    }

    fn first_capture(closure: &TypedExpr) -> Option<BindingId> {
        let expressions = Self::of(closure);
        let bound: HashSet<BindingId> = expressions
            .iter()
            .flat_map(|expr| expr.binders())
            .map(|binder| binder.binding)
            .collect();
        expressions.iter().find_map(|expr| match expr.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) if !bound.contains(&binding) => {
                Some(binding)
            }
            _ => None,
        })
    }
}
