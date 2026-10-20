use super::Expander;
use super::context::DefinitionContext;
use super::problems::ExpandProblems;
use super::similar_name::SimilarName;
use crate::core::{BindingId, BindingInfo, BindingKind, Namespace};
use crate::diagnostic::Diagnostic;
use crate::reader::{Form, Symbol};
use crate::scope::{Phase, Resolution};
use crate::source::Span;

impl Expander<'_> {
    pub(super) fn resolve(
        &self,
        symbol: &Symbol,
        namespace: Namespace,
        span: Span,
    ) -> Result<BindingId, Diagnostic> {
        match self.table.resolve(symbol, namespace, self.phase) {
            Resolution::Bound(binding) => Ok(binding),
            Resolution::Unbound => Err(self.unbound(symbol, namespace, span)),
            Resolution::Ambiguous(bindings) => {
                let definitions = bindings
                    .iter()
                    .filter_map(|binding| self.bindings.get(*binding).span())
                    .collect();
                Err(ExpandProblems::ambiguous(symbol.name(), span, definitions))
            }
        }
    }

    pub(super) fn try_resolve(&self, symbol: &Symbol, namespace: Namespace) -> Option<BindingId> {
        match self.table.resolve(symbol, namespace, self.phase) {
            Resolution::Bound(binding) => Some(binding),
            Resolution::Unbound | Resolution::Ambiguous(_) => None,
        }
    }

    pub(super) fn kind(&self, binding: BindingId) -> BindingKind {
        self.bindings.get(binding).kind()
    }

    pub(super) fn define_local(
        &mut self,
        binder: &Form,
        namespace: Namespace,
        kind: BindingKind,
        context: Option<&DefinitionContext>,
    ) -> Result<BindingId, Diagnostic> {
        self.define(binder, namespace, kind, self.phase, context)
    }

    pub(super) fn define_global(
        &mut self,
        binder: &Form,
        namespace: Namespace,
        kind: BindingKind,
        context: &DefinitionContext,
    ) -> Result<BindingId, Diagnostic> {
        self.define(binder, namespace, kind, Phase::Runtime, Some(context))
    }

    pub(super) fn bind_import(
        &mut self,
        symbol: &Symbol,
        namespace: Namespace,
        binding: BindingId,
        span: Span,
    ) -> Result<(), Diagnostic> {
        self.table
            .bind(symbol, namespace, binding, Phase::Runtime)
            .map_err(|existing| {
                ExpandProblems::duplicate(symbol.name(), span, self.bindings.get(existing).span())
            })
    }

    fn define(
        &mut self,
        binder: &Form,
        namespace: Namespace,
        kind: BindingKind,
        phase: Phase,
        context: Option<&DefinitionContext>,
    ) -> Result<BindingId, Diagnostic> {
        let mut binder = binder.clone();
        if let Some(context) = context {
            context.strip_use_sites(&mut binder);
        }
        let span = binder.span();
        let symbol = binder
            .as_symbol()
            .ok_or_else(|| ExpandProblems::wrong_namespace(&binder.to_string(), "a name", span))?;
        let info = BindingInfo::new(symbol.shared_name(), namespace, kind, Some(span));
        let binding = self.bindings.add(info);
        self.table
            .bind(symbol, namespace, binding, phase)
            .map_err(|existing| {
                ExpandProblems::duplicate(symbol.name(), span, self.bindings.get(existing).span())
            })?;
        Ok(binding)
    }

    fn unbound(&self, symbol: &Symbol, namespace: Namespace, span: Span) -> Diagnostic {
        let candidates = self.table.visible_names(symbol, namespace, self.phase);
        let similar = SimilarName::closest(symbol.name(), &candidates);
        ExpandProblems::unbound(symbol.name(), namespace, span, similar)
    }
}
