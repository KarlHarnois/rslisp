use std::rc::Rc;

use crate::core::{BindingId, BindingKind, FieldLabel, VariantInfo};
use crate::source::Span;
use crate::types::{Type, TypedProgram, VariantDefinition, VariantPayload};

pub struct DeclaredSlot<'d> {
    pub owner: BindingId,
    pub name: &'d Rc<str>,
    pub declared: &'d Type,
    pub span: Span,
    pub function_span: Span,
}

pub struct DeclaredSlots<'p> {
    program: &'p TypedProgram,
}

impl<'p> DeclaredSlots<'p> {
    pub fn new(program: &'p TypedProgram) -> Self {
        Self { program }
    }

    pub fn field(&self, owner: BindingId, label: &FieldLabel) -> Option<DeclaredSlot<'p>> {
        let definition = self.program.structs.get(owner)?;
        let index = definition
            .fields
            .iter()
            .position(|(name, _)| name.as_ref() == label.name.as_ref())?;
        let (name, declared) = &definition.fields[index];
        Some(DeclaredSlot {
            owner,
            name,
            declared,
            span: *definition.field_spans.get(index)?,
            function_span: *definition.function_spans.get(index)?,
        })
    }

    pub fn payload(&self, variant: BindingId, position: usize) -> Option<DeclaredSlot<'p>> {
        let (owner, definition) = self.variant(variant)?;
        Some(DeclaredSlot {
            owner,
            name: &definition.name,
            declared: definition.payload_types().get(position).copied()?,
            span: *definition.payload_spans.get(position)?,
            function_span: *definition.function_spans.get(position)?,
        })
    }

    pub fn payload_label(&self, variant: BindingId, position: usize) -> Option<&'p Rc<str>> {
        let (_, definition) = self.variant(variant)?;
        let VariantPayload::Named(fields) = &definition.payload else {
            return None;
        };
        fields.get(position).map(|(label, _)| label)
    }

    fn variant(&self, variant: BindingId) -> Option<(BindingId, &'p VariantDefinition)> {
        let BindingKind::Variant(_, VariantInfo { owner, index, .. }) =
            self.program.bindings.get(variant).kind()
        else {
            return None;
        };
        let definition = self.program.enums.get(owner)?.variants.get(index)?;
        Some((owner, definition))
    }
}
