use std::collections::HashSet;

use crate::core::BindingId;
use crate::types::{
    TypedBinder, TypedExpr, TypedExprKind, TypedLambda, TypedModule, TypedProgram, TypedSource,
};

pub struct LambdaSource {
    program: TypedProgram,
}

impl LambdaSource {
    pub fn new(text: &str) -> Self {
        Self {
            program: TypedSource::expand(text).typed(),
        }
    }

    pub fn program(&self) -> &TypedProgram {
        &self.program
    }

    pub fn modules_mut(&mut self) -> &mut [TypedModule] {
        &mut self.program.modules
    }

    pub fn lambda(&self, name: &str) -> &TypedLambda {
        match &self.bound_value(name).kind {
            TypedExprKind::Fn(lambda) => lambda,
            _ => panic!("`{name}` is bound to a lambda"),
        }
    }

    pub fn binder(&self, name: &str) -> &TypedBinder {
        self.let_bound(name).0
    }

    pub fn names(&self, bindings: &HashSet<BindingId>) -> HashSet<&str> {
        bindings.iter().map(|binding| self.name(*binding)).collect()
    }

    fn bound_value(&self, name: &str) -> &TypedExpr {
        self.let_bound(name).1
    }

    fn let_bound(&self, name: &str) -> (&TypedBinder, &TypedExpr) {
        self.program
            .modules
            .iter()
            .filter(|module| module.id == self.program.entry)
            .flat_map(|module| &module.definitions)
            .flat_map(|definition| definition.value.let_bindings())
            .find(|(binder, _)| self.name(binder.binding) == name)
            .unwrap_or_else(|| panic!("the source binds `{name}` with `let`"))
    }

    fn name(&self, binding: BindingId) -> &str {
        self.program.bindings.get(binding).name()
    }
}
