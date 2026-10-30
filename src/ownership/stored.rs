use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::aliases::LetAliases;
use super::held::{HeldClosures, RustTemplates};
use super::problems::{FunctionStorage, OwnershipProblems};
use super::slots::DeclaredSlots;
use super::subtree::Subtree;
use crate::core::{BindingId, FieldLabel};
use crate::diagnostic::Diagnostic;
use crate::source::Span;
use crate::types::{
    Type, TypedArgument, TypedDefinition, TypedExpr, TypedExprKind, TypedLambda, TypedLeaf,
    TypedProgram, TypedStructExpr,
};

pub struct StoredClosures<'p> {
    program: &'p TypedProgram,
    slots: FunctionSlots<'p>,
    storing: StoringParameters,
    templates: RustTemplates,
    definitions: Vec<DefinitionTree<'p>>,
}

impl<'p> StoredClosures<'p> {
    pub fn check(program: &mut TypedProgram) -> Vec<Diagnostic> {
        let (diagnostics, stored) = {
            let program = &*program;
            let slots = FunctionSlots::new(program);
            let definitions = DefinitionTree::all_in(program);
            let storing = StoringParameters::collect(&definitions, &slots);
            let closures = StoredClosures {
                program,
                slots,
                storing,
                templates: RustTemplates::of(program),
                definitions,
            };
            (closures.diagnostics(), closures.stored_parameters())
        };
        program.stored_function_parameters = stored;
        diagnostics
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        self.definitions
            .iter()
            .flat_map(|definition| self.capturing_stores(definition))
            .collect()
    }

    fn stored_parameters(&self) -> HashSet<BindingId> {
        self.definitions
            .iter()
            .flat_map(DefinitionTree::functions)
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

    fn capturing_stores(&self, definition: &DefinitionTree) -> Vec<Diagnostic> {
        let mut reported_closures = HashSet::new();
        let held = HeldClosures::new(&definition.aliases, &self.templates);
        definition
            .stores(&self.slots, &self.storing)
            .into_iter()
            .flat_map(|store| store.closures(&held))
            .filter(|store| reported_closures.insert(store.value.span))
            .filter_map(|store| self.capturing_store(store))
            .collect()
    }

    fn capturing_store(&self, store: Store) -> Option<Diagnostic> {
        let captured = Subtree::first_capture(store.value)?;
        Some(OwnershipProblems::capturing_function_field(
            store.value.span,
            self.program.bindings.get(captured).name(),
            &store.slot.storage,
            store.slot.declared,
        ))
    }
}

struct DefinitionTree<'e> {
    function: BindingId,
    lambda: Option<&'e TypedLambda>,
    local_functions: Vec<(BindingId, &'e TypedLambda)>,
    expressions: Vec<&'e TypedExpr>,
    aliases: LetAliases<'e>,
}

impl<'e> DefinitionTree<'e> {
    fn all_in(program: &'e TypedProgram) -> Vec<Self> {
        program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .map(Self::of)
            .collect()
    }

    fn of(definition: &'e TypedDefinition) -> Self {
        let expressions = Subtree::of(&definition.value);
        Self {
            function: definition.binding,
            lambda: match &definition.value.kind {
                TypedExprKind::Fn(lambda) => Some(lambda),
                _ => None,
            },
            aliases: LetAliases::in_expressions(&expressions),
            local_functions: LetAliases::functions_in(&expressions),
            expressions,
        }
    }

    fn stores(&self, slots: &FunctionSlots, storing: &StoringParameters) -> Vec<Store<'e>> {
        self.expressions
            .iter()
            .flat_map(|expr| Store::all_in(expr, slots, storing))
            .map(|store| store.through(&self.aliases))
            .collect()
    }

    fn functions(&self) -> Vec<(BindingId, &'e TypedLambda)> {
        self.lambda
            .map(|lambda| (self.function, lambda))
            .into_iter()
            .chain(self.local_functions.iter().copied())
            .collect()
    }
}

#[derive(Clone)]
struct FunctionSlot {
    storage: FunctionStorage,
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
        if !slot.declared.holds_function() {
            return None;
        }
        Some(FunctionSlot {
            storage: FunctionStorage::Field(Rc::clone(slot.name)),
            declared: slot.span,
        })
    }

    fn payload(&self, variant: BindingId, position: usize) -> Option<FunctionSlot> {
        let slot = self.declared.payload(variant, position)?;
        if !slot.declared.holds_function() {
            return None;
        }
        Some(FunctionSlot {
            storage: FunctionStorage::Payload(Rc::clone(slot.name)),
            declared: slot.span,
        })
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
        let (TypedExprKind::Leaf(TypedLeaf::Global(function))
        | TypedExprKind::Leaf(TypedLeaf::Local(function))) = callee.kind
        else {
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

    fn closures(self, held: &HeldClosures<'_, 'e>) -> Vec<Self> {
        held.in_value(self.value)
            .into_iter()
            .map(|closure| Self::new(closure, self.slot.clone()))
            .collect()
    }

    fn through(self, aliases: &LetAliases<'e>) -> Self {
        Self {
            value: aliases.resolve(self.value),
            slot: self.slot,
        }
    }
}

#[derive(Default)]
struct StoringParameters {
    slots: HashMap<(BindingId, usize), FunctionSlot>,
}

impl StoringParameters {
    fn collect(definitions: &[DefinitionTree<'_>], slots: &FunctionSlots) -> Self {
        let mut storing = Self::default();
        while storing.add_stores(definitions, slots) {}
        storing
    }

    fn slot(&self, function: BindingId, position: usize) -> Option<FunctionSlot> {
        self.slots.get(&(function, position)).cloned()
    }

    fn stores(&self, function: BindingId, position: usize) -> bool {
        self.slots.contains_key(&(function, position))
    }

    fn add_stores(&mut self, definitions: &[DefinitionTree<'_>], slots: &FunctionSlots) -> bool {
        let found: Vec<((BindingId, usize), FunctionSlot)> = definitions
            .iter()
            .flat_map(|definition| self.parameter_stores(definition, slots))
            .filter(|(key, _)| !self.slots.contains_key(key))
            .collect();
        let added = !found.is_empty();
        self.slots.extend(found);
        added
    }

    fn parameter_stores(
        &self,
        definition: &DefinitionTree<'_>,
        slots: &FunctionSlots,
    ) -> Vec<((BindingId, usize), FunctionSlot)> {
        let positions: HashMap<BindingId, (BindingId, usize)> = definition
            .functions()
            .into_iter()
            .flat_map(|(function, lambda)| {
                lambda
                    .params
                    .iter()
                    .enumerate()
                    .map(move |(position, param)| (param.binding, (function, position)))
            })
            .collect();
        definition
            .stores(slots, self)
            .into_iter()
            .filter(|store| matches!(store.value.ty, Type::Function(..)))
            .filter_map(|store| {
                let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = store.value.kind else {
                    return None;
                };
                Some((*positions.get(&binding)?, store.slot))
            })
            .collect()
    }
}
