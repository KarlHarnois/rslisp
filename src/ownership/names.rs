use std::collections::HashSet;

use crate::core::BindingId;
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::types::{Type, TypedDefinition, TypedExprKind, TypedProgram};

use super::problems::OwnershipProblems;

pub struct ChangingFunctionNames;

impl ChangingFunctionNames {
    pub fn check(program: &TypedProgram) -> Diagnostics {
        let rust_trait_methods = Self::rust_trait_methods(program);
        let mut warnings = Diagnostics::default();
        for definition in program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
        {
            if rust_trait_methods.contains(&definition.binding) {
                continue;
            }
            if let Some(warning) = Self::warning(program, definition) {
                warnings.push(warning);
            }
        }
        warnings
    }

    fn rust_trait_methods(program: &TypedProgram) -> HashSet<BindingId> {
        program
            .trait_impls
            .iter()
            .filter(|implementation| {
                program
                    .rust_trait_paths
                    .contains_key(&implementation.trait_binding)
            })
            .flat_map(|implementation| implementation.methods.iter())
            .map(|method| method.binding)
            .collect()
    }

    fn warning(program: &TypedProgram, definition: &TypedDefinition) -> Option<Diagnostic> {
        let TypedExprKind::Fn(lambda) = &definition.value.kind else {
            return None;
        };
        let changes_argument = lambda
            .params
            .iter()
            .any(|param| matches!(&param.ty, Type::Mut(_, _)));
        let info = program.bindings.get(definition.binding);
        let span = info.span().unwrap_or(definition.span);
        (changes_argument && !info.name().ends_with('!'))
            .then(|| OwnershipProblems::missing_bang(info.name(), span))
    }
}
