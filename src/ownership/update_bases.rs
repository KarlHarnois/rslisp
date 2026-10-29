use std::rc::Rc;

use crate::core::FieldLabel;
use crate::types::{CopyRules, StructTable, Type, TypedExpr};

pub struct UpdateBases<'r> {
    structs: &'r StructTable,
    copy: &'r CopyRules<'r>,
}

impl<'r> UpdateBases<'r> {
    pub fn new(structs: &'r StructTable, copy: &'r CopyRules<'r>) -> Self {
        Self { structs, copy }
    }

    pub fn unlisted_fields(
        &self,
        update: &TypedExpr,
        fields: &[(FieldLabel, TypedExpr)],
    ) -> Vec<(Rc<str>, Type)> {
        let Type::Named(structure, arguments) = &update.ty else {
            unreachable!("a struct update has its struct's type")
        };
        let definition = self
            .structs
            .get(*structure)
            .expect("the type checker resolved the updated struct");
        definition
            .instantiated_fields(arguments)
            .into_iter()
            .filter(|(name, _)| !fields.iter().any(|(label, _)| label.name == *name))
            .collect()
    }

    pub fn moves_any(&self, unlisted: &[(Rc<str>, Type)]) -> bool {
        unlisted.iter().any(|field| self.moves(field))
    }

    pub fn moved_fields(&self, unlisted: &[(Rc<str>, Type)]) -> Vec<Rc<str>> {
        unlisted
            .iter()
            .filter(|field| self.moves(field))
            .map(|(name, _)| name.clone())
            .collect()
    }

    fn moves(&self, (_, field_type): &(Rc<str>, Type)) -> bool {
        !self.copy.is_copy(field_type)
    }
}
