use std::collections::HashMap;

use super::checker::TypeChecker;
use super::enums::EnumTable;
use super::problems::TypeProblems;
use super::rust_types::RustTypeTable;
use super::structs::StructTable;
use super::ty::Type;
use super::typed::{TypedDefinition, TypedExprKind, TypedLambda};
use crate::core::{BindingId, CoreModule, ItemKind, TypeExpr, TypeExprKind};
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

struct LabelSpans {
    parameters: Vec<Span>,
    result: Option<Span>,
}

struct OpaquePositions<'c> {
    rust_types: &'c RustTypeTable,
    structs: &'c StructTable,
    enums: &'c EnumTable,
    definitions: &'c HashMap<BindingId, TypedDefinition>,
    signatures: HashMap<BindingId, &'c TypeExpr>,
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
            signatures: OpaquePositions::signatures_in(modules),
        };
        for unnameable in positions.unnameable_in(modules) {
            let shown = self.display().show(&unnameable.opaque);
            self.diagnostics
                .push(TypeProblems::unnameable_type(&shown, unnameable.span));
        }
    }
}

impl OpaquePositions<'_> {
    fn signatures_in(modules: &[CoreModule]) -> HashMap<BindingId, &TypeExpr> {
        modules
            .iter()
            .flat_map(|module| &module.items)
            .filter_map(|item| match &item.kind {
                ItemKind::Signature(binding, signature) => Some((*binding, signature)),
                _ => None,
            })
            .collect()
    }

    fn unnameable_in(&self, modules: &[CoreModule]) -> Vec<UnnameableType> {
        let mut reporting_forms: Vec<Span> = Vec::new();
        let mut found = Vec::new();
        for item in modules.iter().flat_map(|module| &module.items) {
            if Self::reported_by_its_form(&reporting_forms, item.span) {
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

    fn reported_by_its_form(reporting_forms: &[Span], item: Span) -> bool {
        reporting_forms.iter().any(|form| form.contains(item))
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
            .map_or_else(Vec::new, |definition| {
                self.in_function(definition, self.signatures.get(&binding).copied())
            })
    }

    fn in_function(
        &self,
        definition: &TypedDefinition,
        signature: Option<&TypeExpr>,
    ) -> Vec<UnnameableType> {
        let (TypedExprKind::Fn(lambda), Type::Function(_, _, result)) =
            (&definition.value.kind, &definition.value.ty)
        else {
            return Vec::new();
        };
        let spans = LabelSpans::of(lambda, signature);
        lambda
            .params
            .iter()
            .map(|param| &param.ty)
            .zip(spans.parameters)
            .chain(spans.result.map(|span| (result.as_ref(), span)))
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

impl LabelSpans {
    fn of(lambda: &TypedLambda, signature: Option<&TypeExpr>) -> Self {
        signature
            .and_then(Self::written)
            .unwrap_or_else(|| Self::inferred(lambda))
    }

    fn written(signature: &TypeExpr) -> Option<Self> {
        match signature.kind() {
            TypeExprKind::Function(_, parameters, result) => Some(Self {
                parameters: parameters.iter().map(TypeExpr::span).collect(),
                result: Some(result.span()),
            }),
            TypeExprKind::Where(_, function) => Self::written(function),
            _ => None,
        }
    }

    fn inferred(lambda: &TypedLambda) -> Self {
        Self {
            parameters: lambda.params.iter().map(|param| param.span).collect(),
            result: lambda.body.expressions().last().map(|tail| tail.span),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::{LabelSpans, OpaquePositions};
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
        let spans = LabelSpans::written(&bounded).expect("a where wraps a function type");
        assert_eq!(
            spans.parameters,
            vec![fixture.span(SignatureFixture::PARAMETER)]
        );
        assert_eq!(spans.result, Some(fixture.span(SignatureFixture::RESULT)));
    }

    #[test]
    fn a_signature_that_is_not_a_function_labels_nothing() {
        let fixture = SignatureFixture::new();
        assert!(LabelSpans::written(&fixture.hole(0..1)).is_none());
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
