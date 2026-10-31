use std::collections::HashMap;

use super::checker::TypeChecker;
use super::enums::EnumTable;
use super::problems::TypeProblems;
use super::rust_types::RustTypeTable;
use super::structs::StructTable;
use super::ty::Type;
use super::typed::{TypedDefinition, TypedExprKind};
use crate::core::{BindingId, CoreModule, Item, ItemKind};
use crate::source::Span;

struct UnnameableType {
    opaque: Type,
    span: Span,
}

impl UnnameableType {
    fn found(opaque: Option<&Type>, span: Span) -> Option<Self> {
        opaque.map(|opaque| Self {
            opaque: opaque.clone(),
            span,
        })
    }
}

struct OpaquePositions<'c> {
    rust_types: &'c RustTypeTable,
    structs: &'c StructTable,
    enums: &'c EnumTable,
    definitions: &'c HashMap<BindingId, TypedDefinition>,
}

impl TypeChecker {
    pub(super) fn check_opaque_positions(
        &mut self,
        modules: &[CoreModule],
        definitions: &HashMap<BindingId, TypedDefinition>,
    ) {
        let positions = OpaquePositions {
            rust_types: &self.rust_types,
            structs: &self.structs,
            enums: &self.enums,
            definitions,
        };
        for unnameable in positions.unnameable_in(modules) {
            let shown = self.display().show(&unnameable.opaque);
            self.diagnostics
                .push(TypeProblems::unnameable_type(&shown, unnameable.span));
        }
    }
}

impl OpaquePositions<'_> {
    fn unnameable_in(&self, modules: &[CoreModule]) -> Vec<UnnameableType> {
        let mut reporting_forms: Vec<Span> = Vec::new();
        let mut found = Vec::new();
        for item in modules.iter().flat_map(|module| &module.items) {
            if Self::reported_by_its_form(&reporting_forms, item) {
                continue;
            }
            let unnameable = self.unnameable(&item.kind);
            if !unnameable.is_empty() {
                reporting_forms.push(item.span);
            }
            found.extend(unnameable);
        }
        found
    }

    fn reported_by_its_form(reporting_forms: &[Span], item: &Item) -> bool {
        reporting_forms.iter().any(|form| form.contains(item.span))
    }

    fn unnameable(&self, item: &ItemKind) -> Vec<UnnameableType> {
        match item {
            ItemKind::Def(binding, _) => self.in_definition(*binding),
            ItemKind::Struct(binding, ..) => self.in_struct(*binding),
            ItemKind::Enum(binding, ..) => self.in_enum(*binding),
            ItemKind::Signature(..)
            | ItemKind::Derive(..)
            | ItemKind::RustTrait(..)
            | ItemKind::Trait(..)
            | ItemKind::Impl(..)
            | ItemKind::RustType(..) => Vec::new(),
        }
    }

    fn in_definition(&self, binding: BindingId) -> Vec<UnnameableType> {
        self.definitions
            .get(&binding)
            .map_or_else(Vec::new, |definition| self.in_function(definition))
    }

    fn in_function(&self, definition: &TypedDefinition) -> Vec<UnnameableType> {
        let (TypedExprKind::Fn(lambda), Type::Function(_, _, result)) =
            (&definition.value.kind, &definition.value.ty)
        else {
            return Vec::new();
        };
        lambda
            .params
            .iter()
            .map(|param| (&param.ty, param.span))
            .chain([(result.as_ref(), definition.span)])
            .filter_map(|(ty, span)| UnnameableType::found(self.opaque_in_function(ty), span))
            .collect()
    }

    fn in_struct(&self, binding: BindingId) -> Vec<UnnameableType> {
        self.structs
            .get(binding)
            .into_iter()
            .flat_map(|definition| definition.fields.iter().zip(&definition.field_spans))
            .filter_map(|((_, ty), span)| UnnameableType::found(self.opaque_in_function(ty), *span))
            .collect()
    }

    fn in_enum(&self, binding: BindingId) -> Vec<UnnameableType> {
        self.enums
            .get(binding)
            .into_iter()
            .flat_map(|definition| &definition.variants)
            .flat_map(|variant| {
                variant
                    .payload_types()
                    .into_iter()
                    .zip(&variant.payload_spans)
            })
            .filter_map(|(ty, span)| UnnameableType::found(self.opaque_in_function(ty), *span))
            .collect()
    }

    fn opaque_in_function<'t>(&self, ty: &'t Type) -> Option<&'t Type> {
        match ty {
            Type::Function(..) => ty
                .parts()
                .into_iter()
                .find_map(|part| self.opaque_part(part)),
            _ => ty
                .parts()
                .into_iter()
                .find_map(|part| self.opaque_in_function(part)),
        }
    }

    fn opaque_part<'t>(&self, ty: &'t Type) -> Option<&'t Type> {
        if self.is_opaque(ty) {
            return Some(ty);
        }
        ty.parts()
            .into_iter()
            .find_map(|part| self.opaque_part(part))
    }

    fn is_opaque(&self, ty: &Type) -> bool {
        let Type::Named(binding, _) = ty else {
            return false;
        };
        self.rust_types
            .get(*binding)
            .is_some_and(|rust_type| rust_type.is_opaque())
    }
}
