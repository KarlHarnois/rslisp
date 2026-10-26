use std::collections::HashSet;

use crate::core::BindingId;
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::types::{
    ReceiverKind, Type, TypedDefinition, TypedExprKind, TypedProgram, TypedTraitMethod,
};

use super::problems::OwnershipProblems;

pub struct ChangingFunctionNames;

impl ChangingFunctionNames {
    pub fn check(program: &TypedProgram) -> Diagnostics {
        let impl_methods = Self::impl_methods(program);
        let default_methods = Self::default_methods(program);
        let mut warnings = Diagnostics::default();
        for definition in program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
        {
            if impl_methods.contains(&definition.binding)
                || default_methods.contains(&definition.binding)
            {
                continue;
            }
            if let Some(warning) = Self::function_warning(program, definition) {
                warnings.push(warning);
            }
        }
        for method in program
            .trait_definitions
            .iter()
            .flat_map(|definition| &definition.methods)
        {
            if let Some(warning) = Self::trait_method_warning(program, method) {
                warnings.push(warning);
            }
        }
        warnings
    }

    fn impl_methods(program: &TypedProgram) -> HashSet<BindingId> {
        program
            .trait_impls
            .iter()
            .flat_map(|implementation| implementation.methods.iter())
            .map(|method| method.binding)
            .collect()
    }

    fn default_methods(program: &TypedProgram) -> HashSet<BindingId> {
        program
            .trait_definitions
            .iter()
            .flat_map(|definition| definition.methods.iter())
            .filter_map(|method| method.default)
            .collect()
    }

    fn function_warning(
        program: &TypedProgram,
        definition: &TypedDefinition,
    ) -> Option<Diagnostic> {
        let TypedExprKind::Fn(lambda) = &definition.value.kind else {
            return None;
        };
        let changes_argument = lambda
            .params
            .iter()
            .any(|param| matches!(&param.ty, Type::Mut(_, _)));
        Self::changed_name_warning(
            program,
            definition.binding,
            definition.span,
            changes_argument,
        )
    }

    fn trait_method_warning(
        program: &TypedProgram,
        method: &TypedTraitMethod,
    ) -> Option<Diagnostic> {
        let definition = Self::definition(program, method.binding)?;
        let changes_argument =
            method.receiver == Some(ReceiverKind::Mutable) || Self::takes_mut_parameter(definition);
        Self::changed_name_warning(program, method.binding, definition.span, changes_argument)
    }

    fn definition(program: &TypedProgram, binding: BindingId) -> Option<&TypedDefinition> {
        program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .find(|definition| definition.binding == binding)
    }

    fn takes_mut_parameter(definition: &TypedDefinition) -> bool {
        match &definition.scheme.body {
            Type::Function(_, parameters, _) => parameters
                .iter()
                .any(|parameter| matches!(parameter, Type::Mut(..))),
            _ => false,
        }
    }

    fn changed_name_warning(
        program: &TypedProgram,
        binding: BindingId,
        definition_span: crate::source::Span,
        changes_argument: bool,
    ) -> Option<Diagnostic> {
        let info = program.bindings.get(binding);
        let span = info.span().unwrap_or(definition_span);
        (changes_argument && !info.name().ends_with('!'))
            .then(|| OwnershipProblems::missing_bang(info.name(), span))
    }
}
