use super::Expander;
use super::context::DefinitionContext;
use super::problems::ExpandProblems;
use crate::core::{BindingId, BindingInfo, BindingKind, ModuleId, Namespace};
use crate::diagnostic::SimilarName;
use crate::diagnostic::{Diagnostic, Label};
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
        match self.tables.table.resolve(symbol, namespace, self.phase) {
            Resolution::Bound(binding) => Ok(binding),
            Resolution::Unbound => Err(self.unbound(symbol, namespace, span)),
            Resolution::Ambiguous(bindings) => {
                let definitions = bindings
                    .iter()
                    .filter_map(|binding| self.tables.bindings.get(*binding).span())
                    .collect();
                Err(ExpandProblems::ambiguous(symbol.name(), span, definitions))
            }
        }
    }

    pub(super) fn try_resolve(&self, symbol: &Symbol, namespace: Namespace) -> Option<BindingId> {
        match self.tables.table.resolve(symbol, namespace, self.phase) {
            Resolution::Bound(binding) => Some(binding),
            Resolution::Unbound | Resolution::Ambiguous(_) => None,
        }
    }

    pub(super) fn kind(&self, binding: BindingId) -> BindingKind {
        self.tables.bindings.get(binding).kind()
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
        let binding = self.define(binder, namespace, kind, Phase::Runtime, Some(context))?;
        self.note_global(binding, binder);
        Ok(binding)
    }

    pub(super) fn bind_import(
        &mut self,
        symbol: &Symbol,
        namespace: Namespace,
        binding: BindingId,
        span: Span,
    ) -> Result<(), Diagnostic> {
        self.tables
            .table
            .bind(symbol, namespace, binding, Phase::Runtime)
            .map_err(|existing| {
                ExpandProblems::duplicate(
                    symbol.name(),
                    span,
                    self.tables.bindings.get(existing).span(),
                )
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
        Self::reject_keyword_binder(symbol.name(), span)?;
        let info = BindingInfo::new(symbol.shared_name(), kind, Some(span));
        let binding = self.tables.bindings.add(info);
        self.tables
            .table
            .bind(symbol, namespace, binding, phase)
            .map_err(|existing| {
                ExpandProblems::duplicate(
                    symbol.name(),
                    span,
                    self.tables.bindings.get(existing).span(),
                )
            })?;
        Ok(binding)
    }

    pub(super) fn unbound(&self, symbol: &Symbol, namespace: Namespace, span: Span) -> Diagnostic {
        let candidates: Vec<_> = self
            .tables
            .table
            .visible_name_bindings(symbol, namespace, self.phase)
            .into_iter()
            .filter(|(name, binding)| self.is_public(name, namespace, *binding))
            .map(|(name, _)| name)
            .collect();
        let similar = SimilarName::closest(symbol.name(), &candidates);
        let definition = self.introduced_name(span, symbol.name());
        let primary = definition.unwrap_or(span);
        let diagnostic = ExpandProblems::unbound(symbol.name(), namespace, primary, similar);
        match (definition, self.tables.macro_origins.get(&span)) {
            (Some(_), Some(origin)) => diagnostic
                .with_note(format!("in this expansion of `{}`", origin.name))
                .with_label(Label::new(span, "in this macro call")),
            _ => diagnostic,
        }
    }

    fn is_public(&self, name: &str, namespace: Namespace, binding: BindingId) -> bool {
        match self.definition_module(binding) {
            Some(module) if Some(module) != self.current_module => {
                self.modules.get(module).exported(name, namespace) == Some(binding)
            }
            _ => true,
        }
    }

    pub(super) fn definition_module(&self, binding: BindingId) -> Option<ModuleId> {
        match self.kind(binding) {
            BindingKind::Global(module)
            | BindingKind::Macro(module)
            | BindingKind::Struct(module)
            | BindingKind::Enum(module)
            | BindingKind::Variant(module, _)
            | BindingKind::Trait(module)
            | BindingKind::RustType(module) => Some(module),
            _ => None,
        }
    }
}
