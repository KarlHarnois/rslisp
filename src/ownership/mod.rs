mod changes;
mod checks;
mod definitions;
mod functions;
mod modes;
mod moves;
mod names;
mod problems;
mod rewrite;
mod usage;

use crate::diagnostic::Diagnostics;
use crate::types::{CopyRules, InferredCopies, TypedExprKind, TypedProgram};
use checks::{BorrowedResults, OwnershipNeeds};
use functions::FunctionTable;
use modes::ModeInference;
use moves::MoveChecker;
use names::ChangingFunctionNames;
use rewrite::ModeRewrite;

pub use changes::InPlaceChanges;

pub struct OwnershipChecker;

impl OwnershipChecker {
    pub fn check(program: &mut TypedProgram) -> Result<InferredCopies, Diagnostics> {
        let functions = FunctionTable::collect(program);
        let modes = ModeInference::new(
            &functions,
            CopyRules::new(&program.traits, &program.generics, &program.structs),
        )
        .run();
        let rewrite = ModeRewrite::new(&functions, &modes);
        rewrite.apply(program);
        let mut diagnostics = Diagnostics::default();
        let mut inferred_copies = InferredCopies::default();
        for definition in program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
        {
            if let TypedExprKind::Fn(lambda) = &definition.value.kind {
                let checker = MoveChecker::new(
                    CopyRules::new(&program.traits, &program.generics, &program.structs),
                    &program.bindings,
                    &program.template_borrows,
                    &program.generics,
                );
                let report = checker.check(lambda);
                inferred_copies.extend(report.inferred_copies);
                report
                    .diagnostics
                    .into_iter()
                    .for_each(|diagnostic| diagnostics.push(diagnostic));
                BorrowedResults::check(definition, &program.bindings)
                    .into_iter()
                    .for_each(|diagnostic| diagnostics.push(diagnostic));
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
}
