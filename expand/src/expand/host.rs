use super::Expander;
use super::failed_definition::DeclaredName;
use super::problems::ExpandProblems;
use crate::core::{BindingId, BindingKind, Declaration, ModulePath, Namespace};
use crate::diagnostic::Diagnostic;
use crate::interpret::{CompileTimeHost, Value};
use crate::modules::ModuleSources;
use crate::reader::{Form, Symbol};
use crate::scope::{Phase, Resolution, ScopeSet};
use crate::source::Span;

impl CompileTimeHost for Expander<'_> {
    fn global_value(&mut self, binding: BindingId, span: Span) -> Result<Value, Diagnostic> {
        match self.kind(binding) {
            BindingKind::CodeBuiltin(builtin) => Ok(Value::Builtin(builtin)),
            BindingKind::Global(_) => self.evaluate_global(binding, span),
            _ => Err(ExpandProblems::not_at_compile_time(
                self.tables.bindings.get(binding).name(),
                span,
            )),
        }
    }

    fn definition_symbol(
        &self,
        term: &Symbol,
        name: String,
        span: Span,
    ) -> Result<Symbol, Diagnostic> {
        let binding = self.bound_term(term, span)?;
        Ok(Symbol::with_scopes(name, self.definition_scopes(binding)))
    }

    fn definition_call(&self, term: &Symbol, span: Span) -> Result<Option<Form>, Diagnostic> {
        let binding = self.bound_term(term, span)?;
        Ok(self.defining_call(binding))
    }

    fn type_definition_call(&self, term: &Symbol) -> Option<Form> {
        let binding = self.namespace_binding(term, Namespace::Type)?;
        self.defining_call(binding)
    }

    fn expand_call(&mut self, call: Form) -> Option<Form> {
        let binding = self.macro_head(&call)?;
        self.discarded_expansion(binding, call)
    }

    fn type_definition_symbol(&self, term: &Symbol, name: String) -> Symbol {
        let scopes = self
            .namespace_binding(term, Namespace::Type)
            .map_or_else(ScopeSet::default, |binding| self.definition_scopes(binding));
        Symbol::with_scopes(name, scopes)
    }

    fn add_module_note(&mut self, key: &str, note: Form) {
        let module = self
            .current_module
            .expect("macro code runs while a module expands");
        self.tables.module_notes.add(module, key, note);
    }

    fn module_notes(&self, context: &Symbol, key: &str) -> Vec<Form> {
        self.modules
            .with_body_scopes(context.scopes())
            .map_or_else(Vec::new, |module| self.tables.module_notes.of(module, key))
    }

    fn declare_definition(&mut self, name: Form, declaration: Declaration) {
        let introduction = *self
            .introductions
            .last()
            .expect("macro code runs inside a macro call");
        self.declared_definitions
            .push(DeclaredName::new(name, declaration, introduction));
    }

    fn same_binding(&self, left: &Symbol, right: &Symbol) -> bool {
        let pairs: Vec<(Resolution, Resolution)> = [Namespace::Value, Namespace::Type]
            .into_iter()
            .map(|namespace| {
                (
                    self.tables.table.resolve(left, namespace, Phase::Runtime),
                    self.tables.table.resolve(right, namespace, Phase::Runtime),
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
    fn definition_scopes(&self, binding: BindingId) -> ScopeSet {
        if let Some(module) = self.definition_module(binding) {
            return self.modules.get(module).body_scopes();
        }
        match self.kind(binding) {
            BindingKind::KernelForm(_) => self.prelude_scopes(),
            _ => ScopeSet::default(),
        }
    }

    fn prelude_scopes(&self) -> ScopeSet {
        let prelude = self
            .modules
            .find(&ModulePath::new(ModuleSources::PRELUDE))
            .expect("macro code runs after the prelude module is registered");
        self.modules.get(prelude).body_scopes()
    }

    fn bound_term(&self, term: &Symbol, span: Span) -> Result<BindingId, Diagnostic> {
        self.term_binding(term)
            .ok_or_else(|| self.unbound(term, Namespace::Value, span))
    }

    pub(super) fn term_binding(&self, term: &Symbol) -> Option<BindingId> {
        [Namespace::Value, Namespace::Type]
            .into_iter()
            .find_map(|namespace| self.namespace_binding(term, namespace))
    }

    pub(super) fn namespace_binding(
        &self,
        term: &Symbol,
        namespace: Namespace,
    ) -> Option<BindingId> {
        match self.tables.table.resolve(term, namespace, Phase::Runtime) {
            Resolution::Bound(binding) => Some(binding),
            _ => None,
        }
    }
}
