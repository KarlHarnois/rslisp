use std::collections::{HashMap, HashSet};

use super::checker::TypeChecker;
use super::enums::EnumTable;
use super::problems::TypeProblems;
use super::rust_types::RustTypeTable;
use super::structs::StructTable;
use super::ty::Type;
use super::typed::{TypedDefinition, TypedExprKind, TypedLambda};
use crate::core::{BindingId, CoreModule, Item, ItemKind, TraitMethod, TypeExpr, TypeExprKind};
use crate::diagnostic::Diagnostic;
use crate::source::Span;

struct UnnameableType {
    opaque: Type,
    span: Span,
    place: UnnameablePlace,
}

#[derive(Clone, Copy)]
enum UnnameablePlace {
    FunctionType,
    StoredValue,
}

#[derive(Clone, Copy)]
enum FunctionLabels<'f> {
    Written(&'f [TypeExpr], &'f TypeExpr),
    Inferred(&'f TypedLambda),
}

struct OpaquePositions<'c> {
    rust_types: &'c RustTypeTable,
    structs: &'c StructTable,
    enums: &'c EnumTable,
    definitions: &'c HashMap<BindingId, TypedDefinition>,
    modules: &'c [CoreModule],
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
            modules,
        };
        for unnameable in positions.unnameable() {
            let shown = self.display().show(&unnameable.opaque);
            self.unnameable_types.push(unnameable.diagnostic(&shown));
        }
    }
}

impl UnnameableType {
    fn diagnostic(&self, shown: &str) -> Diagnostic {
        match self.place {
            UnnameablePlace::FunctionType => TypeProblems::unnameable_type(shown, self.span),
            UnnameablePlace::StoredValue => TypeProblems::stored_unnameable_type(shown, self.span),
        }
    }
}

impl OpaquePositions<'_> {
    fn unnameable(&self) -> Vec<UnnameableType> {
        let mut reporting_forms: Vec<Span> = Vec::new();
        let mut found = Vec::new();
        for item in self.items() {
            if Self::reported_by_its_form(&reporting_forms, item.span) {
                continue;
            }
            let unnameable = self.in_item(&item.kind);
            if !unnameable.is_empty() {
                reporting_forms.push(item.span);
            }
            found.extend(unnameable);
        }
        Self::once_per_span(found)
    }

    fn items(&self) -> impl Iterator<Item = &Item> {
        self.modules.iter().flat_map(|module| &module.items)
    }

    fn reported_by_its_form(reporting_forms: &[Span], item: Span) -> bool {
        reporting_forms.iter().any(|form| form.contains(item))
    }

    fn once_per_span(found: Vec<UnnameableType>) -> Vec<UnnameableType> {
        let mut reported = HashSet::new();
        found
            .into_iter()
            .filter(|unnameable| reported.insert(unnameable.span))
            .collect()
    }

    fn in_item(&self, item: &ItemKind) -> Vec<UnnameableType> {
        match item {
            ItemKind::Def(binding, _) => self.in_definition(*binding),
            ItemKind::Struct(binding, ..) => self.in_struct(*binding),
            ItemKind::Enum(binding, ..) => self.in_enum(*binding),
            ItemKind::Trait(.., methods) => self.in_trait(methods),
            ItemKind::Signature(..)
            | ItemKind::Derive(..)
            | ItemKind::RustTrait(..)
            | ItemKind::Impl(..)
            | ItemKind::RustType(..) => Vec::new(),
        }
    }

    fn in_definition(&self, binding: BindingId) -> Vec<UnnameableType> {
        self.definitions
            .get(&binding)
            .map_or_else(Vec::new, |definition| self.in_function(binding, definition))
    }

    fn in_function(&self, binding: BindingId, definition: &TypedDefinition) -> Vec<UnnameableType> {
        let TypedExprKind::Fn(lambda) = &definition.value.kind else {
            return Vec::new();
        };
        self.in_signature(&definition.value.ty, || self.labels(binding, lambda))
    }

    fn in_trait(&self, methods: &[TraitMethod]) -> Vec<UnnameableType> {
        methods
            .iter()
            .flat_map(|method| self.in_trait_method(method))
            .collect()
    }

    fn in_trait_method(&self, method: &TraitMethod) -> Vec<UnnameableType> {
        self.definitions
            .get(&method.binding)
            .zip(FunctionLabels::written(&method.signature))
            .map_or_else(Vec::new, |(declaration, labels)| {
                self.in_signature(&declaration.value.ty, || labels)
            })
    }

    fn in_signature<'f>(
        &self,
        ty: &Type,
        labels: impl Fn() -> FunctionLabels<'f>,
    ) -> Vec<UnnameableType> {
        let Type::Function(_, parameters, result) = ty else {
            return Vec::new();
        };
        let in_parameters = parameters
            .iter()
            .enumerate()
            .filter_map(|(position, parameter)| {
                self.unnameable_at(parameter, || labels().parameter(position))
            });
        let in_result = self.unnameable_at(result, || labels().result());
        in_parameters.chain(in_result).collect()
    }

    fn in_struct(&self, binding: BindingId) -> Vec<UnnameableType> {
        self.structs
            .get(binding)
            .into_iter()
            .flat_map(|definition| definition.fields.iter().zip(&definition.field_spans))
            .filter_map(|((_, ty), span)| self.stored_at(ty, *span))
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
            .filter_map(|(ty, span)| self.stored_at(ty, *span))
            .collect()
    }

    fn stored_at(&self, ty: &Type, span: Span) -> Option<UnnameableType> {
        self.unnameable_at(ty, || Some(span))
            .or_else(|| self.stored_opaque_at(ty, span))
    }

    fn stored_opaque_at(&self, ty: &Type, span: Span) -> Option<UnnameableType> {
        Some(UnnameableType {
            opaque: self.opaque_part(ty)?.clone(),
            span,
            place: UnnameablePlace::StoredValue,
        })
    }

    fn unnameable_at(
        &self,
        ty: &Type,
        label: impl FnOnce() -> Option<Span>,
    ) -> Option<UnnameableType> {
        let opaque = self.opaque_in_function(ty)?;
        Some(UnnameableType {
            opaque: opaque.clone(),
            span: label()?,
            place: UnnameablePlace::FunctionType,
        })
    }

    fn labels<'f>(&'f self, binding: BindingId, lambda: &'f TypedLambda) -> FunctionLabels<'f> {
        self.signature(binding)
            .and_then(FunctionLabels::written)
            .unwrap_or(FunctionLabels::Inferred(lambda))
    }

    fn signature(&self, binding: BindingId) -> Option<&TypeExpr> {
        self.items().find_map(|item| match &item.kind {
            ItemKind::Signature(signed, signature) if *signed == binding => Some(signature),
            _ => None,
        })
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

impl<'f> FunctionLabels<'f> {
    fn parameter(&self, position: usize) -> Option<Span> {
        match self {
            Self::Written(parameters, _) => parameters.get(position).map(TypeExpr::span),
            Self::Inferred(lambda) => lambda.params.get(position).map(|param| param.span),
        }
    }

    fn result(&self) -> Option<Span> {
        match self {
            Self::Written(_, result) => Some(result.span()),
            Self::Inferred(lambda) => lambda.body.expressions().last().map(|tail| tail.span),
        }
    }

    fn written(signature: &'f TypeExpr) -> Option<Self> {
        match signature.kind() {
            TypeExprKind::Function(_, parameters, result) => {
                Some(Self::Written(parameters, result))
            }
            TypeExprKind::Where(_, function) => Self::written(function),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::{FunctionLabels, OpaquePositions};
    use crate::core::{ClosureKind, TypeExpr, TypeExprKind};
    use crate::source::{SourceId, Span};

    struct SignatureFixture {
        source: SourceId,
    }

    impl SignatureFixture {
        const PARAMETER: Range<usize> = 5..6;
        const RESULT: Range<usize> = 8..9;

        fn new() -> Self {
            Self {
                source: SourceId::new(0),
            }
        }

        fn span(&self, range: Range<usize>) -> Span {
            Span::new(self.source, range)
        }

        fn hole(&self, range: Range<usize>) -> TypeExpr {
            TypeExpr::new(TypeExprKind::Hole, self.span(range))
        }

        fn function(&self) -> TypeExpr {
            let parameters = vec![self.hole(Self::PARAMETER)];
            let result = Box::new(self.hole(Self::RESULT));
            TypeExpr::new(
                TypeExprKind::Function(ClosureKind::Fn, parameters, result),
                self.span(0..10),
            )
        }
    }

    #[test]
    fn a_where_signature_labels_the_parts_of_its_function() {
        let fixture = SignatureFixture::new();
        let bounded = TypeExpr::new(
            TypeExprKind::Where(Vec::new(), Box::new(fixture.function())),
            fixture.span(0..20),
        );
        let labels = FunctionLabels::written(&bounded).expect("a where wraps a function type");
        assert_eq!(
            labels.parameter(0),
            Some(fixture.span(SignatureFixture::PARAMETER))
        );
        assert_eq!(labels.parameter(1), None);
        assert_eq!(
            labels.result(),
            Some(fixture.span(SignatureFixture::RESULT))
        );
    }

    #[test]
    fn a_signature_that_is_not_a_function_labels_nothing() {
        let fixture = SignatureFixture::new();
        assert!(FunctionLabels::written(&fixture.hole(0..1)).is_none());
    }

    #[test]
    fn only_an_item_inside_an_earlier_reporting_form_is_skipped() {
        let source = SourceId::new(0);
        let reporting_form = Span::new(source, 10..50);
        let inside = Span::new(source, 20..30);
        let enclosing = Span::new(source, 0..60);
        assert!(OpaquePositions::reported_by_its_form(
            &[reporting_form],
            inside
        ));
        assert!(!OpaquePositions::reported_by_its_form(
            &[reporting_form],
            enclosing
        ));
    }
}
