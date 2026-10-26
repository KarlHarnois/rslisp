use std::collections::{HashMap, HashSet};

use crate::core::{BindingId, StructDerives};
use crate::diagnostic::Diagnostics;
use crate::expand::Program;
use crate::ownership::{InPlaceChanges, OwnershipChecker};
use crate::types::{InferredCopies, InstantiationReport, Pinning, Type, TypeChecker, TypedProgram};

pub struct ProgramChecker;

struct CheckedProgram {
    program: TypedProgram,
    inferred_copies: InferredCopies,
    instantiations: InstantiationReport,
}

impl ProgramChecker {
    pub fn check(program: &Program) -> Result<TypedProgram, Diagnostics> {
        let first = Self::checked(
            program,
            &HashSet::new(),
            &InferredCopies::default(),
            &HashMap::new(),
            false,
        )?;
        Self::finish(program, first)
    }

    fn finish(program: &Program, first: CheckedProgram) -> Result<TypedProgram, Diagnostics> {
        let blocked = Self::inferred_copy_changed(&first.program);
        let pins = Self::pins(&first.instantiations);
        if blocked.is_empty()
            && first.inferred_copies.is_empty()
            && pins.is_empty()
            && first.program.literal_generics.is_empty()
        {
            return Ok(first.program);
        }
        let checked = Self::checked(program, &blocked, &first.inferred_copies, &pins, true)?;
        Self::pin_until_stable(program, first.inferred_copies, blocked, pins, checked)
    }

    fn pin_until_stable(
        program: &Program,
        inferred_copies: InferredCopies,
        blocked: HashSet<BindingId>,
        mut pins: HashMap<BindingId, Type>,
        mut checked: CheckedProgram,
    ) -> Result<TypedProgram, Diagnostics> {
        loop {
            let next: HashMap<BindingId, Type> = Self::pins(&checked.instantiations)
                .into_iter()
                .filter(|(binding, _)| !pins.contains_key(binding))
                .collect();
            if next.is_empty() {
                return Ok(checked.program);
            }
            pins.extend(next);
            checked = Self::checked(program, &blocked, &inferred_copies, &pins, true)?;
        }
    }

    fn pins(report: &InstantiationReport) -> HashMap<BindingId, Type> {
        Pinning::of(report).pins().iter().cloned().collect()
    }

    fn checked(
        program: &Program,
        changed_structs: &HashSet<BindingId>,
        inferred_copies: &InferredCopies,
        pinned: &HashMap<BindingId, Type>,
        default_literal_parameters: bool,
    ) -> Result<CheckedProgram, Diagnostics> {
        match Self::typed_and_owned(
            program,
            changed_structs,
            inferred_copies,
            pinned,
            default_literal_parameters,
        ) {
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
        pinned: &HashMap<BindingId, Type>,
        default_literal_parameters: bool,
    ) -> Result<CheckedProgram, Diagnostics> {
        let mut typed = TypeChecker::check(
            program,
            changed_structs,
            inferred_copies,
            pinned,
            default_literal_parameters,
        )?;
        let instantiations = InstantiationReport::of(program, &typed);
        let inferred_copies = OwnershipChecker::check(&mut typed)?;
        Ok(CheckedProgram {
            program: typed,
            inferred_copies,
            instantiations,
        })
    }

    fn inferred_copy_changed(typed: &TypedProgram) -> HashSet<BindingId> {
        InPlaceChanges::of(typed)
            .into_iter()
            .filter(|binding| {
                typed.structs.get(*binding).is_some_and(|definition| {
                    definition.written_derive.is_none()
                        && definition.derives.contains(&StructDerives::COPY)
                }) || typed.enums.get(*binding).is_some_and(|definition| {
                    definition.written_derive.is_none()
                        && definition.derives.contains(&StructDerives::COPY)
                })
            })
            .collect()
    }
}
