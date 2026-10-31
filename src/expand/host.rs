use super::Expander;
use super::problems::ExpandProblems;
use crate::core::{BindingId, BindingKind, Namespace};
use crate::diagnostic::Diagnostic;
use crate::interpret::{CompileTimeHost, Value};
use crate::reader::{Form, Symbol};
use crate::scope::{Phase, Resolution, ScopeSet};
use crate::source::Span;

impl CompileTimeHost for Expander<'_> {
    fn global_value(&mut self, binding: BindingId, span: Span) -> Result<Value, Diagnostic> {
        let name = self.bindings.get(binding).name().to_string();
        match self.kind(binding) {
            BindingKind::CodeBuiltin(builtin) => Ok(Value::Builtin(builtin)),
            BindingKind::Global(_) => self.evaluate_global(binding, span),
            _ => Err(ExpandProblems::not_at_compile_time(&name, span)),
        }
    }

    fn definition_symbol(
        &self,
        term: &Symbol,
        name: String,
        span: Span,
    ) -> Result<Symbol, Diagnostic> {
        let binding = self
            .term_binding(term)
            .ok_or_else(|| self.unbound(term, Namespace::Value, span))?;
        Ok(Symbol::with_scopes(
            name,
            self.definition_scopes(term, binding),
        ))
    }

    fn type_definition_symbol(&self, term: &Symbol, name: String) -> Symbol {
        let scopes = self
            .namespace_binding(term, Namespace::Type)
            .map_or_else(ScopeSet::default, |binding| {
                self.definition_scopes(term, binding)
            });
        Symbol::with_scopes(name, scopes)
    }

    fn add_module_note(&mut self, key: &str, note: Form) {
        let module = self
            .current_module
            .expect("macro code runs while a module expands");
        self.module_notes.add(module, key, note);
    }

    fn module_notes(&self, context: &Symbol, key: &str) -> Vec<Form> {
        self.modules
            .with_body_scopes(context.scopes())
            .map_or_else(Vec::new, |module| self.module_notes.of(module, key))
    }

    fn same_binding(&self, left: &Symbol, right: &Symbol) -> bool {
        let pairs: Vec<(Resolution, Resolution)> = [Namespace::Value, Namespace::Type]
            .into_iter()
            .map(|namespace| {
                (
                    self.table.resolve(left, namespace, Phase::Runtime),
                    self.table.resolve(right, namespace, Phase::Runtime),
                )
            })
            .collect();
        let bound_together = pairs.iter().any(|pair| {
            matches!(pair, (Resolution::Bound(left_binding), Resolution::Bound(right_binding))
                if left_binding == right_binding)
        });
        let unbound_everywhere = pairs
            .iter()
            .all(|pair| matches!(pair, (Resolution::Unbound, Resolution::Unbound)));
        bound_together || (unbound_everywhere && left.name() == right.name())
    }
}

impl Expander<'_> {
    fn definition_scopes(&self, term: &Symbol, binding: BindingId) -> ScopeSet {
        if let Some(module) = self.definition_module(binding) {
            return self.modules.get(module).body_scopes();
        }
        match self.kind(binding) {
            BindingKind::KernelForm(_) => term.scopes().clone(),
            _ => ScopeSet::default(),
        }
    }

    fn term_binding(&self, term: &Symbol) -> Option<BindingId> {
        [Namespace::Value, Namespace::Type]
            .into_iter()
            .find_map(|namespace| self.namespace_binding(term, namespace))
    }

    fn namespace_binding(&self, term: &Symbol, namespace: Namespace) -> Option<BindingId> {
        match self.table.resolve(term, namespace, Phase::Runtime) {
            Resolution::Bound(binding) => Some(binding),
            _ => None,
        }
    }
}
