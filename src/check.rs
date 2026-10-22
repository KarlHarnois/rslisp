use std::collections::HashSet;

use crate::core::{BindingId, StructDerives};
use crate::diagnostic::Diagnostics;
use crate::expand::Program;
use crate::ownership::{InPlaceChanges, OwnershipChecker};
use crate::types::{InferredCopies, TypeChecker, TypedProgram};

pub struct ProgramChecker;

struct CheckedProgram {
    program: TypedProgram,
    inferred_copies: InferredCopies,
}

impl ProgramChecker {
    pub fn check(program: &Program) -> Result<TypedProgram, Diagnostics> {
        let first = Self::checked(program, &HashSet::new(), &InferredCopies::default())?;
        let blocked = Self::inferred_copy_changed(&first.program);
        if blocked.is_empty() && first.inferred_copies.is_empty() {
            return Ok(first.program);
        }
        Self::checked(program, &blocked, &first.inferred_copies).map(|checked| checked.program)
    }

    fn checked(
        program: &Program,
        changed_structs: &HashSet<BindingId>,
        inferred_copies: &InferredCopies,
    ) -> Result<CheckedProgram, Diagnostics> {
        match Self::typed_and_owned(program, changed_structs, inferred_copies) {
            Ok(mut checked) => {
                checked.program.warnings.extend(program.warnings.clone());
                Ok(checked)
            }
            Err(mut diagnostics) => {
                diagnostics.extend(program.warnings.clone());
                Err(diagnostics)
            }
        }
    }

    fn typed_and_owned(
        program: &Program,
        changed_structs: &HashSet<BindingId>,
        inferred_copies: &InferredCopies,
    ) -> Result<CheckedProgram, Diagnostics> {
        let mut program = TypeChecker::check(program, changed_structs, inferred_copies)?;
        let inferred_copies = OwnershipChecker::check(&mut program)?;
        Ok(CheckedProgram {
            program,
            inferred_copies,
        })
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
