use crate::types::{FieldPlace, TypedExpr};

pub enum StructUpdateOrder<'a> {
    FieldsBeforePlace(FieldPlace<'a>),
    BaseFirst,
}

impl<'a> StructUpdateOrder<'a> {
    pub fn of(base: &'a TypedExpr) -> Self {
        match base.field_place() {
            Some(place) => Self::FieldsBeforePlace(place),
            None => Self::BaseFirst,
        }
    }
}
