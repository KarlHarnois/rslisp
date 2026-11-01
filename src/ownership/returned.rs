use std::collections::HashSet;

use super::lent_borrows::LentBorrows;
use super::local_borrows::{BorrowHolder, LocalBorrows, ParameterOwner};
use super::problems::{BorrowedStorage, OwnershipProblems};
use super::slots::DeclaredSlots;
use crate::core::{BindingId, FieldLabel, PrimitiveType};
use crate::diagnostic::Diagnostic;
use crate::types::{
    RustType, Type, TypeDisplay, TypedDefinition, TypedExprKind, TypedLambda, TypedProgram,
};

pub struct ReturnedBorrows<'p> {
    program: &'p TypedProgram,
    slots: DeclaredSlots<'p>,
    owners_of_borrowing_closures: HashSet<BindingId>,
}

impl<'p> ReturnedBorrows<'p> {
    pub fn new(program: &'p TypedProgram) -> Self {
        Self {
            program,
            slots: DeclaredSlots::new(program),
            owners_of_borrowing_closures: BorrowingClosures { program }.owners(),
        }
    }

    pub fn check_definition(&self, definition: &TypedDefinition) -> Vec<Diagnostic> {
        self.checked_functions(definition)
            .into_iter()
            .filter_map(|(lambda, owner)| self.check(lambda, owner))
            .chain(self.lent_borrows(definition))
            .collect()
    }

    fn lent_borrows(&self, definition: &TypedDefinition) -> Vec<Diagnostic> {
        if self.owns_borrowing_closures(definition) {
            return LentBorrows::new(self.program).check_definition(definition);
        }
        Vec::new()
    }

    fn checked_functions<'d>(
        &self,
        definition: &'d TypedDefinition,
    ) -> Vec<(&'d TypedLambda, ParameterOwner)> {
        let function =
            Self::function_of(definition).map(|lambda| (lambda, ParameterOwner::Function));
        let closures = self
            .closures_to_check(definition)
            .into_iter()
            .map(|closure| (closure, ParameterOwner::Closure));
        function.into_iter().chain(closures).collect()
    }

    fn function_of(definition: &TypedDefinition) -> Option<&TypedLambda> {
        match &definition.value.kind {
            TypedExprKind::Fn(lambda) => Some(lambda),
            _ => None,
        }
    }

    fn closures_to_check<'d>(&self, definition: &'d TypedDefinition) -> Vec<&'d TypedLambda> {
        if self.owns_borrowing_closures(definition) {
            return LocalBorrows::closures_in(&definition.value);
        }
        Vec::new()
    }

    fn owns_borrowing_closures(&self, definition: &TypedDefinition) -> bool {
        self.owners_of_borrowing_closures
            .contains(&definition.binding)
    }

    fn check(&self, lambda: &TypedLambda, owner: ParameterOwner) -> Option<Diagnostic> {
        let tail = LocalBorrows::tail_of(&lambda.body)?;
        let borrows = LocalBorrows::dropped_in(lambda, owner);
        let found = borrows.returned(tail)?;
        let storage = found.holders.iter().find_map(|holder| self.storage(holder));
        Some(match storage {
            Some(storage) => {
                OwnershipProblems::stored_local_borrow(found.span, &found.dropped, &storage)
            }
            None => OwnershipProblems::local_borrow(found.span, &found.dropped),
        })
    }

    fn storage(&self, holder: &BorrowHolder) -> Option<BorrowedStorage> {
        match holder {
            BorrowHolder::Field(owner, label) => self.field_storage(*owner, label),
            BorrowHolder::Payload(variant, position) => self.payload_storage(*variant, *position),
        }
    }

    fn field_storage(&self, owner: BindingId, label: &FieldLabel) -> Option<BorrowedStorage> {
        let slot = self.slots.field(owner, label)?;
        Some(BorrowedStorage::Field {
            field: slot.name.to_string(),
            owner: self.name(slot.owner),
            owned: self.owned(slot.declared)?,
            declared: slot.span,
        })
    }

    fn payload_storage(&self, variant: BindingId, position: usize) -> Option<BorrowedStorage> {
        let slot = self.slots.payload(variant, position)?;
        let owner = self.name(slot.owner);
        let owned = self.owned(slot.declared)?;
        Some(match self.slots.payload_label(variant, position) {
            Some(field) => BorrowedStorage::NamedPayload {
                field: field.to_string(),
                variant: slot.name.to_string(),
                owner,
                owned,
                declared: slot.span,
            },
            None => BorrowedStorage::Payload {
                variant: slot.name.to_string(),
                owner,
                owned,
                declared: slot.span,
            },
        })
    }

    fn owned(&self, declared: &Type) -> Option<String> {
        let owned = self.owned_type(declared)?;
        let declares_borrow = declared.holds_reference() && !owned.holds_reference();
        declares_borrow.then(|| self.display().show(&owned))
    }

    fn owned_type(&self, ty: &Type) -> Option<Type> {
        match ty {
            Type::Ref(_, viewed) | Type::Mut(_, viewed) => self.owned_view(viewed),
            Type::Named(binding, arguments) => {
                Some(Type::Named(*binding, self.owned_types(arguments)?))
            }
            other => Some(other.clone()),
        }
    }

    fn owned_view(&self, viewed: &Type) -> Option<Type> {
        match viewed {
            Type::Primitive(PrimitiveType::Str) => Some(Type::Primitive(PrimitiveType::String)),
            Type::Named(slice, elements) if self.program.rust_types.is_slice(*slice) => {
                Some(Type::Named(self.vector()?, self.owned_types(elements)?))
            }
            other => self.owned_type(other),
        }
    }

    fn owned_types(&self, types: &[Type]) -> Option<Vec<Type>> {
        types.iter().map(|ty| self.owned_type(ty)).collect()
    }

    fn vector(&self) -> Option<BindingId> {
        self.program
            .rust_types
            .binding_with_template(RustType::VECTOR_TEMPLATE)
    }

    fn name(&self, binding: BindingId) -> String {
        self.program.bindings.get(binding).name().to_string()
    }

    fn display(&self) -> TypeDisplay<'p> {
        TypeDisplay::new(&self.program.bindings, &self.program.generics)
    }
}

struct BorrowingClosures<'p> {
    program: &'p TypedProgram,
}

impl BorrowingClosures<'_> {
    fn owners(&self) -> HashSet<BindingId> {
        self.program
            .closure_types
            .iter()
            .filter(|closure| self.can_return_borrow(&closure.ty))
            .map(|closure| closure.definition)
            .collect()
    }

    fn can_return_borrow(&self, closure: &Type) -> bool {
        matches!(closure, Type::Function(_, _, result)
            if result.holds_reference() || self.names_borrowing_type(result))
    }

    fn names_borrowing_type(&self, ty: &Type) -> bool {
        matches!(ty, Type::Named(binding, _) if self.program.definition_holds_borrow(*binding))
            || ty
                .parts()
                .into_iter()
                .any(|part| self.names_borrowing_type(part))
    }
}
