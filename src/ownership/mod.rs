mod aliases;
mod borrowing_lambdas;
mod callers;
mod changes;
mod checks;
mod definitions;
mod escaping_lambdas;
mod functions;
mod held;
mod kinds;
mod modes;
mod moved_values;
mod moves;
mod mutation;
mod names;
mod parameter_uses;
mod problems;
mod refusals;
mod repeats;
mod returned;
mod rewrite;
mod slices;
mod slots;
mod stored;
mod stored_definitions;
mod subtree;
mod tail_calls;
mod update_bases;
mod update_order;
mod usage;
mod values;
mod worklist;

use std::collections::HashMap;

use crate::core::{BindingArena, BindingId};
use crate::diagnostic::Diagnostics;
use crate::types::{
    CopyRules, InferredCopies, PendingModeCheck, Pipelines, TypedExprKind, TypedProgram,
};
use borrowing_lambdas::BorrowingLambdas;
use checks::OwnershipNeeds;
use escaping_lambdas::EscapingLambdas;
use functions::{FunctionTable, ParamMode};
use kinds::{ClosureKinds, KindReport, KindSources};
use modes::ModeInference;
use moves::{MoveChecker, MoveReport};
use names::ChangingFunctionNames;
use problems::OwnershipProblems;
use returned::ReturnedBorrows;
use rewrite::ModeRewrite;
use slices::ShrinkSelection;
use stored::StoredClosures;
use stored_definitions::StoredDefinitions;
use values::FunctionValueModes;

pub use changes::InPlaceChanges;
pub use mutation::ChangedValues;
pub use tail_calls::TailCalls;
pub use update_order::StructUpdateOrder;

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
        let copy = CopyRules::new(
            &program.traits,
            &program.generics,
            &program.structs,
            &program.enums,
        );
        let mut rewrite = ModeRewrite::new(&functions, &modes, &slices, &copy);
        rewrite.apply(program);
        FunctionValueModes::apply(program, rewrite.function_params())
            .into_iter()
            .for_each(|diagnostic| diagnostics.push(diagnostic));
        program.escaping_lambdas = EscapingLambdas::in_program(program);
        StoredClosures::check(program)
            .into_iter()
            .for_each(|diagnostic| diagnostics.push(diagnostic));
        program.borrowing_lambdas = BorrowingLambdas::in_program(program);
        let kinds = ClosureKinds::new(KindSources {
            traits: &program.traits,
            generics: &program.generics,
            structs: &program.structs,
            enums: &program.enums,
            bindings: &program.bindings,
            template_borrows: &program.template_borrows,
            function_parameters: ClosureKinds::function_parameters(program),
            rust_parameter_kinds: ClosureKinds::rust_parameter_kinds(program),
            written_parameter_kinds: ClosureKinds::written_parameter_kinds(program),
            borrowing_lambdas: &program.borrowing_lambdas,
        });
        let report = kinds.run(&mut program.modules);
        Self::apply_kinds(program, report, &mut diagnostics);
        let mut inferred_copies = InferredCopies::default();
        let pipelines = Pipelines::new(&program.rust_types);
        let stored_definitions = StoredDefinitions::of(program);
        let returned_borrows = ReturnedBorrows::new(program);
        for definition in program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
        {
            returned_borrows
                .check_definition(definition)
                .into_iter()
                .for_each(|diagnostic| diagnostics.push(diagnostic));
            let report = match &definition.value.kind {
                TypedExprKind::Fn(lambda) => {
                    Self::move_checker(program, pipelines, &stored_definitions)
                        .check(definition.binding, lambda)
                }
                _ if stored_definitions.contains(definition.binding) => {
                    Self::move_checker(program, pipelines, &stored_definitions)
                        .check_value(&definition.value)
                }
                _ => MoveReport::default(),
            };
            inferred_copies.extend(report.inferred_copies);
            report
                .diagnostics
                .into_iter()
                .for_each(|diagnostic| diagnostics.push(diagnostic));
            let needs = OwnershipNeeds::new(rewrite.function_params(), pipelines)
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

    fn move_checker<'a>(
        program: &'a TypedProgram,
        pipelines: Pipelines<'a>,
        stored_definitions: &'a StoredDefinitions,
    ) -> MoveChecker<'a> {
        MoveChecker::new(
            CopyRules::new(
                &program.traits,
                &program.generics,
                &program.structs,
                &program.enums,
            ),
            &program.structs,
            &program.bindings,
            &program.template_borrows,
            &program.generics,
            pipelines,
            stored_definitions,
        )
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
