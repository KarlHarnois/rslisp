use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::types::{Type, TypedDefinition, TypedExprKind, TypedProgram};

use super::problems::OwnershipProblems;

pub struct ChangingFunctionNames;

impl ChangingFunctionNames {
    pub fn check(program: &TypedProgram) -> Diagnostics {
        let mut warnings = Diagnostics::default();
        for definition in program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
        {
            if let Some(warning) = Self::warning(program, definition) {
                warnings.push(warning);
            }
        }
        warnings
    }

    fn warning(program: &TypedProgram, definition: &TypedDefinition) -> Option<Diagnostic> {
        let TypedExprKind::Fn(lambda) = &definition.value.kind else {
            return None;
        };
        let changes_argument = lambda
            .params
            .iter()
            .any(|param| matches!(&param.ty, Type::Mut(_)));
        let name = program.bindings.get(definition.binding).name();
        (changes_argument && !name.ends_with('!'))
            .then(|| OwnershipProblems::missing_bang(name, definition.span))
    }
}
