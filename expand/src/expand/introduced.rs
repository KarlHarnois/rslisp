use super::Expander;
use crate::core::{BindingId, BindingKind, ModuleId, Namespace};
use crate::reader::Form;

impl Expander<'_> {
    pub(super) fn note_global(&mut self, binding: BindingId, binder: &Form) {
        if self.carries_introduction_scope(binder) {
            self.tables.bindings.mark_introduced(binding);
        }
        if matches!(self.kind(binding), BindingKind::Macro(_)) {
            self.tables.global_macros.push((binding, binder.clone()));
        }
    }

    pub(super) fn record_answering_macros(&mut self, module: ModuleId) {
        let (macros, others): (Vec<_>, Vec<_>) = std::mem::take(&mut self.tables.global_macros)
            .into_iter()
            .partition(|(binding, _)| self.kind(*binding) == BindingKind::Macro(module));
        self.tables.global_macros = others;
        for (answering, name) in macros {
            if let Some(function) = self.answered_function(answering, name) {
                self.tables
                    .bindings
                    .record_answering_macro(function, answering);
            }
        }
    }

    fn carries_introduction_scope(&self, binder: &Form) -> bool {
        binder.as_symbol().is_some_and(|symbol| {
            self.tables
                .introduction_scopes
                .iter()
                .any(|scope| symbol.scopes().contains(*scope))
        })
    }

    pub(super) fn answered_global(
        &mut self,
        module: ModuleId,
        binding: BindingId,
        name: &Form,
    ) -> Option<BindingId> {
        if self.kind(binding) != BindingKind::Macro(module) {
            return None;
        }
        self.answered_function(binding, name.clone())
    }

    fn answered_function(&mut self, answering: BindingId, name: Form) -> Option<BindingId> {
        let expansion = self.discarded_expansion(answering, name)?;
        let function = self.try_resolve(expansion.as_symbol()?, Namespace::Value)?;
        self.tables
            .bindings
            .is_introduced(function)
            .then_some(function)
    }
}
