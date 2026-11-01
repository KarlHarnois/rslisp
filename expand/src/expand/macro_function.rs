use super::Expander;
use crate::core::{BindingId, BindingKind, Namespace};
use crate::reader::{Form, FormKind, Symbol};
use crate::scope::Resolution;
use crate::source::Span;

impl Expander<'_> {
    const TRIAL_LEFT: &'static str = "suggestion-left";
    const TRIAL_RIGHT: &'static str = "suggestion-right";

    pub(super) fn macro_function(
        &mut self,
        binding: BindingId,
        reference: &Symbol,
        span: Span,
    ) -> Option<BindingId> {
        let (trial, left, right) = Self::trial_form(reference, span);
        let output = self.discarded_expansion(binding, trial)?;
        let function = self.output_function(&output, &left, &right)?;
        self.resolves_to(function, reference)
    }

    fn trial_form(reference: &Symbol, span: Span) -> (Form, Symbol, Symbol) {
        let left = Symbol::with_scopes(Self::TRIAL_LEFT, reference.scopes().clone());
        let right = Symbol::with_scopes(Self::TRIAL_RIGHT, reference.scopes().clone());
        let form = |symbol: &Symbol| Form::new(FormKind::Symbol(symbol.clone()), span);
        let items = vec![form(reference), form(&left), form(&right)];
        (Form::new(FormKind::List(items), span), left, right)
    }

    fn output_function(&self, output: &Form, left: &Symbol, right: &Symbol) -> Option<BindingId> {
        let [head, first, second] = output.as_list()? else {
            return None;
        };
        if first.as_symbol()?.name() != left.name() || second.as_symbol()?.name() != right.name() {
            return None;
        }
        let binding = self.try_resolve(head.as_symbol()?, Namespace::Value)?;
        matches!(self.kind(binding), BindingKind::Global(_)).then_some(binding)
    }

    fn resolves_to(&self, function: BindingId, reference: &Symbol) -> Option<BindingId> {
        let name = self.tables.bindings.get(function).name();
        let candidate = Symbol::with_scopes(name, reference.scopes().clone());
        match self
            .tables
            .table
            .resolve(&candidate, Namespace::Value, self.phase)
        {
            Resolution::Bound(resolved) if resolved == function => Some(function),
            _ => None,
        }
    }
}
