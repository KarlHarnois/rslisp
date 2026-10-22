use std::collections::HashSet;

use crate::core::{BindingId, StructDerives};
use crate::diagnostic::Diagnostics;
use crate::expand::Program;
use crate::ownership::{InPlaceChanges, OwnershipChecker};
use crate::types::{TypeChecker, TypedProgram};

pub struct ProgramChecker;

impl ProgramChecker {
    pub fn check(program: &Program) -> Result<TypedProgram, Diagnostics> {
        let typed = Self::typed_and_owned(program, &HashSet::new())?;
        let blocked = Self::inferred_copy_changed(&typed);
        if blocked.is_empty() {
            return Ok(typed);
        }
        Self::typed_and_owned(program, &blocked)
    }

    fn typed_and_owned(
        program: &Program,
        changed_structs: &HashSet<BindingId>,
    ) -> Result<TypedProgram, Diagnostics> {
        let mut typed = TypeChecker::check(program, changed_structs)?;
        OwnershipChecker::check(&mut typed)?;
        Ok(typed)
    }

    fn inferred_copy_changed(typed: &TypedProgram) -> HashSet<BindingId> {
        InPlaceChanges::of(typed)
            .into_iter()
            .filter(|binding| {
                typed.structs.get(*binding).is_some_and(|definition| {
                    definition.written_derive.is_none()
                        && definition.derives.contains(&StructDerives::COPY)
                })
            })
            .collect()
    }
}
