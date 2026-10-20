mod checks;
mod functions;
mod modes;
mod moves;
mod problems;
mod rewrite;
mod usage;

use crate::diagnostic::Diagnostics;
use crate::types::{CopyRules, TypedExprKind, TypedProgram};
use checks::{BorrowedResults, OwnershipNeeds};
use functions::FunctionTable;
use modes::ModeInference;
use moves::MoveChecker;
use rewrite::ModeRewrite;

pub struct OwnershipChecker;

impl OwnershipChecker {
    pub fn check(program: &mut TypedProgram) -> Result<(), Diagnostics> {
        let functions = FunctionTable::collect(program);
        let modes = ModeInference::new(
            &functions,
            CopyRules::new(&program.traits, &program.generics),
        )
        .run();
        let rewrite = ModeRewrite::new(&functions, &modes);
        rewrite.apply(program);
        let mut diagnostics = Diagnostics::default();
        for definition in program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
        {
            if let TypedExprKind::Fn(lambda) = &definition.value.kind {
                let checker = MoveChecker::new(
                    CopyRules::new(&program.traits, &program.generics),
                    &program.bindings,
                );
                checker
                    .check(lambda)
                    .into_iter()
                    .for_each(|diagnostic| diagnostics.push(diagnostic));
                BorrowedResults::check(definition, &program.bindings)
                    .into_iter()
                    .for_each(|diagnostic| diagnostics.push(diagnostic));
            }
            let needs = OwnershipNeeds::new(rewrite.function_params()).check(&definition.value);
            needs
                .into_iter()
                .for_each(|diagnostic| diagnostics.push(diagnostic));
        }
        diagnostics.into_result(())
    }
}
