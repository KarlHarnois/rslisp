use std::collections::HashSet;

use crate::core::{BindingId, ReferenceKind};
use crate::types::{Borrow, Type, TypedExpr, TypedExprKind, TypedProgram, TypedStructExpr};

pub struct InPlaceChanges;

impl InPlaceChanges {
    pub fn of(program: &TypedProgram) -> HashSet<BindingId> {
        let mut changed = HashSet::new();
        for definition in program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
        {
            Self::written_mut_types(&definition.scheme.body, &mut changed);
            Self::expression(&definition.value, &mut changed);
        }
        changed.retain(|binding| {
            program.structs.get(*binding).is_some() || program.enums.get(*binding).is_some()
        });
        changed
    }

    fn expression(expr: &TypedExpr, changed: &mut HashSet<BindingId>) {
        Self::written_mut_types(&expr.ty, changed);
        for binder in expr.binders() {
            Self::written_mut_types(&binder.ty, changed);
        }
        if let TypedExprKind::Call(_, arguments) | TypedExprKind::Recur(arguments) = &expr.kind {
            for argument in arguments {
                if matches!(argument.borrow, Borrow::Mutable | Borrow::Reborrow) {
                    Self::place(&argument.value, changed);
                }
            }
        }
        if let TypedExprKind::Borrow(ReferenceKind::Mutable, value) = &expr.kind {
            Self::place(value, changed);
        }
        for child in expr.children() {
            Self::expression(child, changed);
        }
    }

    fn place(place: &TypedExpr, changed: &mut HashSet<BindingId>) {
        match &place.kind {
            TypedExprKind::Struct(TypedStructExpr::Field(owner, _)) => {
                Self::changed_type(&owner.ty, changed);
                Self::place(owner, changed);
            }
            _ => Self::changed_type(&place.ty, changed),
        }
    }

    fn written_mut_types(ty: &Type, changed: &mut HashSet<BindingId>) {
        if let Type::Mut(_, inner) = ty {
            Self::changed_type(inner, changed);
        }
        for part in ty.parts() {
            Self::written_mut_types(part, changed);
        }
    }

    fn changed_type(ty: &Type, changed: &mut HashSet<BindingId>) {
        let mut resolved = ty;
        while let Type::Ref(_, inner) | Type::Mut(_, inner) = resolved {
            resolved = inner;
        }
        if let Type::Named(binding, _) = resolved {
            changed.insert(*binding);
        }
    }
}
