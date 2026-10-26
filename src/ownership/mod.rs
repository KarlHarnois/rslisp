mod changes;
mod checks;
mod definitions;
mod functions;
mod kinds;
mod modes;
mod moves;
mod mutation;
mod names;
mod problems;
mod rewrite;
mod slices;
mod usage;
mod values;

use std::collections::HashMap;

use crate::core::{BindingArena, BindingId};
use crate::diagnostic::Diagnostics;
use crate::types::{CopyRules, InferredCopies, PendingModeCheck, TypedExprKind, TypedProgram};
use checks::{OwnershipNeeds, ReturnedBorrows};
use functions::{FunctionTable, ParamMode};
use kinds::{ClosureKinds, KindReport, KindSources};
use modes::ModeInference;
use moves::MoveChecker;
use names::ChangingFunctionNames;
use problems::OwnershipProblems;
use rewrite::ModeRewrite;
use slices::ShrinkSelection;
use values::FunctionValueModes;

pub use changes::InPlaceChanges;
pub use mutation::ChangedValues;

pub struct OwnershipChecker;

impl OwnershipChecker {
    pub fn check(program: &mut TypedProgram) -> Result<InferredCopies, Diagnostics> {
        let functions = FunctionTable::collect(program);
        let modes = ModeInference::new(
            &functions,
            CopyRules::new(
                &program.traits,
                &program.generics,
                &program.structs,
                &program.enums,
            ),
        )
        .run();
        program.borrowed_parameters = modes
            .iter()
            .filter(|(_, mode)| matches!(mode, ParamMode::Ref | ParamMode::Mut))
            .map(|(binding, _)| *binding)
            .collect();
        let mut diagnostics = Diagnostics::default();
        Self::check_written_modes(
            &program.pending_mode_checks,
            &functions,
            &modes,
            &program.bindings,
            &mut diagnostics,
        );
        let slices = ShrinkSelection::of(program, &functions, &modes)
            .map(|selection| selection.run())
            .unwrap_or_default();
        let mut rewrite = ModeRewrite::new(&functions, &modes, &slices);
        rewrite.apply(program);
        FunctionValueModes::apply(program, rewrite.function_params())
            .into_iter()
            .for_each(|diagnostic| diagnostics.push(diagnostic));
        let kinds = ClosureKinds::new(KindSources {
            traits: &program.traits,
            generics: &program.generics,
            structs: &program.structs,
            enums: &program.enums,
            bindings: &program.bindings,
            function_parameters: ClosureKinds::function_parameters(program),
            rust_parameter_kinds: ClosureKinds::rust_parameter_kinds(program),
            written_parameter_kinds: ClosureKinds::written_parameter_kinds(program),
        });
        let report = kinds.run(&mut program.modules);
        Self::apply_kinds(program, report, &mut diagnostics);
        let mut inferred_copies = InferredCopies::default();
        for definition in program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
        {
            if let TypedExprKind::Fn(lambda) = &definition.value.kind {
                let checker = MoveChecker::new(
                    CopyRules::new(
                        &program.traits,
                        &program.generics,
                        &program.structs,
                        &program.enums,
                    ),
                    &program.bindings,
                    &program.template_borrows,
                    &program.generics,
                    definition.binding,
                );
                let report = checker.check(lambda);
                inferred_copies.extend(report.inferred_copies);
                report
                    .diagnostics
                    .into_iter()
                    .for_each(|diagnostic| diagnostics.push(diagnostic));
                if let crate::types::Type::Function(_, _, result) = &definition.value.ty {
                    ReturnedBorrows::check(lambda, result)
                        .into_iter()
                        .for_each(|diagnostic| diagnostics.push(diagnostic));
                }
            }
            let needs = OwnershipNeeds::new(rewrite.function_params())
                .check(definition.binding, &definition.value);
            needs
                .into_iter()
                .for_each(|diagnostic| diagnostics.push(diagnostic));
        }
        let warnings = ChangingFunctionNames::check(program);
        if diagnostics.has_errors() {
            diagnostics.extend(warnings);
        } else {
            program.warnings.extend(warnings);
        }
        diagnostics.into_result(inferred_copies)
    }

    fn apply_kinds(program: &mut TypedProgram, report: KindReport, diagnostics: &mut Diagnostics) {
        program.mutable_function_bindings = report.mutable_bindings;
        report
            .diagnostics
            .into_iter()
            .for_each(|diagnostic| diagnostics.push(diagnostic));
    }

    fn check_written_modes(
        checks: &[PendingModeCheck],
        functions: &FunctionTable,
        modes: &HashMap<BindingId, ParamMode>,
        bindings: &BindingArena,
        diagnostics: &mut Diagnostics,
    ) {
        for check in checks {
            let Some(info) = functions.get(check.callee) else {
                continue;
            };
            let Some(slot) = info.params.get(check.position) else {
                continue;
            };
            let mode = modes.get(&slot.binding).copied().unwrap_or(ParamMode::Own);
            if mode.accepts(check.kind) {
                continue;
            }
            diagnostics.push(OwnershipProblems::written_mode_mismatch(
                bindings.get(slot.binding).name(),
                check.kind,
                mode,
                check.span,
            ));
        }
    }
}
