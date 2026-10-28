use std::rc::Rc;

use crate::core::{BindingId, BindingKind, FieldLabel, VariantInfo};
use crate::source::Span;
use crate::types::{Type, TypedProgram, VariantPayload};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SlotShape {
    Field,
    PositionalPayload,
    NamedPayload,
}

pub struct DeclaredSlot<'d> {
    pub owner: BindingId,
    pub name: &'d Rc<str>,
    pub declared: &'d Type,
    pub span: Span,
    pub shape: SlotShape,
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
            shape: SlotShape::Field,
        })
    }

    pub fn payload(&self, variant: BindingId, position: usize) -> Option<DeclaredSlot<'p>> {
        let BindingKind::Variant(_, VariantInfo { owner, index, .. }) =
            self.program.bindings.get(variant).kind()
        else {
            return None;
        };
        let definition = self.program.enums.get(owner)?.variants.get(index)?;
        Some(DeclaredSlot {
            owner,
            name: &definition.name,
            declared: definition.payload_types().get(position).copied()?,
            span: *definition.payload_spans.get(position)?,
            shape: Self::payload_shape(&definition.payload)?,
        })
    }

    pub fn positional_payload(
        &self,
        variant: BindingId,
        position: usize,
    ) -> Option<DeclaredSlot<'p>> {
        self.payload(variant, position)
            .filter(|slot| slot.shape == SlotShape::PositionalPayload)
    }

    fn payload_shape(payload: &VariantPayload) -> Option<SlotShape> {
        match payload {
            VariantPayload::None => None,
            VariantPayload::Positional(_) => Some(SlotShape::PositionalPayload),
            VariantPayload::Named(_) => Some(SlotShape::NamedPayload),
        }
    }
}
