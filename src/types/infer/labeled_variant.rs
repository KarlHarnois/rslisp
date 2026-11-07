use std::rc::Rc;

use crate::core::{BindingId, Expr, FieldLabel};
use crate::diagnostic::Diagnostic;
use crate::source::Span;
use crate::types::checker::TypeChecker;
use crate::types::enums::VariantPayload;
use crate::types::problems::{MismatchContext, TypeProblems};
use crate::types::{FieldOrder, Type, TypedExpr, TypedExprKind};

use super::VariantSite;

impl TypeChecker {
    pub(super) fn infer_labeled_variant(
        &mut self,
        binding: BindingId,
        fields: &[(FieldLabel, Expr)],
        expected: Option<&Type>,
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        let variant_name = self.bindings.get(binding).name().to_string();
        let site = self.variant_site(binding, expected);
        let declared = self.declared_fields(&site, &variant_name, &fields[0].0)?;
        let mut typed = Vec::new();
        let mut order = Vec::new();
        for (given, (_, value)) in fields.iter().enumerate() {
            let position = Self::field_position(&variant_name, &declared, fields, given)?;
            let field_type = &declared[position].1;
            typed.push(self.infer_against(value, field_type, MismatchContext::Plain)?);
            order.push(position);
        }
        Self::require_every_field(&variant_name, &declared, fields, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Variant(binding, typed, FieldOrder::of(order)),
            ty: Type::Named(site.info.owner, site.type_arguments),
            span,
        })
    }

    fn declared_fields(
        &self,
        site: &VariantSite,
        variant_name: &str,
        first_label: &FieldLabel,
    ) -> Result<Vec<(Rc<str>, Type)>, Diagnostic> {
        let definition = self
            .enums
            .get(site.info.owner)
            .expect("a variant owner is an enum");
        let VariantPayload::Named(fields) = &definition.variants[site.info.index].payload else {
            return Err(TypeProblems::label_on_positional_payload(
                variant_name,
                first_label,
                site.info.arity,
            ));
        };
        let labels = fields.iter().map(|(label, _)| label.clone());
        Ok(labels.zip(site.payload.iter().cloned()).collect())
    }
}
