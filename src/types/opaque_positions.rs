use std::collections::HashMap;

use super::checker::TypeChecker;
use super::enums::EnumTable;
use super::problems::TypeProblems;
use super::rust_types::RustTypeTable;
use super::structs::StructTable;
use super::ty::Type;
use super::typed::{TypedDefinition, TypedExprKind, TypedLambda};
use crate::core::{BindingId, CoreModule, Item, ItemKind, TypeExpr, TypeExprKind};
use crate::source::Span;

struct UnnameableType {
    opaque: Type,
    span: Span,
}

struct FunctionLabels<'f> {
    lambda: &'f TypedLambda,
    written: Option<(&'f [TypeExpr], &'f TypeExpr)>,
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
            self.diagnostics
                .push(TypeProblems::unnameable_type(&shown, unnameable.span));
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
        found
    }

    fn items(&self) -> impl Iterator<Item = &Item> {
        self.modules.iter().flat_map(|module| &module.items)
    }

    fn reported_by_its_form(reporting_forms: &[Span], item: Span) -> bool {
        reporting_forms.iter().any(|form| form.contains(item))
    }

    fn in_item(&self, item: &ItemKind) -> Vec<UnnameableType> {
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
            .map_or_else(Vec::new, |definition| self.in_function(binding, definition))
    }

    fn in_function(&self, binding: BindingId, definition: &TypedDefinition) -> Vec<UnnameableType> {
        let (TypedExprKind::Fn(lambda), Type::Function(_, _, result)) =
            (&definition.value.kind, &definition.value.ty)
        else {
            return Vec::new();
        };
        let in_parameters = lambda
            .params
            .iter()
            .enumerate()
            .filter_map(|(position, param)| {
                self.unnameable_at(&param.ty, || {
                    self.labels(binding, lambda).parameter(position)
                })
            });
        let in_result = self.unnameable_at(result, || self.labels(binding, lambda).result());
        in_parameters.chain(in_result).collect()
    }

    fn in_struct(&self, binding: BindingId) -> Vec<UnnameableType> {
        self.structs
            .get(binding)
            .into_iter()
            .flat_map(|definition| definition.fields.iter().zip(&definition.field_spans))
            .filter_map(|((_, ty), span)| self.unnameable_at(ty, || Some(*span)))
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
            .filter_map(|(ty, span)| self.unnameable_at(ty, || Some(*span)))
            .collect()
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
        })
    }

    fn labels<'f>(&'f self, binding: BindingId, lambda: &'f TypedLambda) -> FunctionLabels<'f> {
        FunctionLabels {
            lambda,
            written: self.signature(binding).and_then(FunctionLabels::written),
        }
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

impl FunctionLabels<'_> {
    fn parameter(&self, position: usize) -> Option<Span> {
        match self.written {
            Some((parameters, _)) => parameters.get(position).map(TypeExpr::span),
            None => self.lambda.params.get(position).map(|param| param.span),
        }
    }

    fn result(&self) -> Option<Span> {
        match self.written {
            Some((_, result)) => Some(result.span()),
            None => self.lambda.body.expressions().last().map(|tail| tail.span),
        }
    }

    fn written(signature: &TypeExpr) -> Option<(&[TypeExpr], &TypeExpr)> {
        match signature.kind() {
            TypeExprKind::Function(_, parameters, result) => Some((parameters, result)),
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
        let (parameters, result) =
            FunctionLabels::written(&bounded).expect("a where wraps a function type");
        let parameter_spans: Vec<Span> = parameters.iter().map(TypeExpr::span).collect();
        assert_eq!(
            parameter_spans,
            vec![fixture.span(SignatureFixture::PARAMETER)]
        );
        assert_eq!(result.span(), fixture.span(SignatureFixture::RESULT));
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
