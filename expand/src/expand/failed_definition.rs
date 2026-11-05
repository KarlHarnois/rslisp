use super::Expander;
use super::context::DefinitionContext;
use super::rest_of_body::RestOfBody;
use crate::core::{
    BindingId, BindingKind, BodyItem, Declaration, Expr, ExprKind, Leaf, ModuleId, Namespace,
};
use crate::diagnostic::Diagnostic;
use crate::reader::{Form, Symbol};
use crate::scope::{Phase, Scope};

pub(super) struct DeclaredName {
    name: Form,
    declaration: Declaration,
    introduction: Scope,
}

impl DeclaredName {
    pub(super) fn new(name: Form, declaration: Declaration, introduction: Scope) -> Self {
        Self {
            name,
            declaration,
            introduction,
        }
    }

    fn binder(&self) -> Form {
        let mut binder = self.name.clone();
        binder.flip_scope(self.introduction);
        binder
    }
}

impl Expander<'_> {
    pub(super) fn apply_definer(
        &mut self,
        definer: BindingId,
        call: Form,
        context: &mut DefinitionContext,
    ) -> Result<Form, Diagnostic> {
        self.declared_definitions.clear();
        let expansion = self.apply_macro(definer, call, Some(context));
        if expansion.is_err() {
            self.bind_failed_names(context);
        }
        expansion
    }

    pub(super) fn failed_macro_use(&self, form: &Form) -> Option<Expr> {
        if !self.tables.globals.has_failed_definitions() {
            return None;
        }
        let head = form.as_list()?.first()?.as_symbol()?;
        let binding = self.try_resolve(head, Namespace::Value)?;
        self.tables
            .globals
            .is_failed_macro_definition(binding)
            .then(|| Expr::new(ExprKind::Leaf(Leaf::Var(binding)), form.span()))
    }

    pub(super) fn bind_over_failed(
        &mut self,
        symbol: &Symbol,
        namespace: Namespace,
        binding: BindingId,
        phase: Phase,
    ) -> Result<(), BindingId> {
        match self.tables.table.bind(symbol, namespace, binding, phase) {
            Err(existing) if self.tables.globals.is_failed_definition(existing) => {
                self.forget_failed(symbol, namespace, existing);
                self.tables.table.bind(symbol, namespace, binding, phase)
            }
            bound => bound,
        }
    }

    fn forget_failed(&mut self, symbol: &Symbol, namespace: Namespace, failed: BindingId) {
        self.tables.table.unbind(symbol, namespace, failed);
        self.tables.globals.forget_failed_definition(failed);
    }

    pub(super) fn bind_failed_body_name(
        &mut self,
        failure: Expr,
        rest_of_body: &mut RestOfBody,
        context: &DefinitionContext,
    ) -> Result<BodyItem, Diagnostic> {
        let Some(name) = self.declared_value_name() else {
            return Ok(BodyItem::Expr(failure));
        };
        let scope = self.fresh_local_scope();
        rest_of_body.add_scope(scope);
        let binder = self.lower_binder(&Self::scoped(&name, scope), Some(context))?;
        Ok(BodyItem::Let(binder, failure))
    }

    fn declared_value_name(&self) -> Option<Form> {
        self.declared_definitions
            .iter()
            .find(|declared| declared.declaration.namespace() == Namespace::Value)
            .map(DeclaredName::binder)
    }

    fn bind_failed_names(&mut self, context: &DefinitionContext) {
        let module = self
            .current_module
            .expect("a top-level call expands inside a module");
        let declared = std::mem::take(&mut self.declared_definitions);
        for name in &declared {
            self.bind_failed_name(&name.binder(), name.declaration, module, context);
        }
        self.declared_definitions = declared;
    }

    fn bind_failed_name(
        &mut self,
        name: &Form,
        declaration: Declaration,
        module: ModuleId,
        context: &DefinitionContext,
    ) {
        let namespace = declaration.namespace();
        let kind = Self::failed_kind(namespace, module);
        if let Ok(binding) = self.define_unrecorded_global(name, namespace, kind, context) {
            self.tables
                .globals
                .record_failed_definition(binding, declaration);
        }
    }

    fn failed_kind(namespace: Namespace, module: ModuleId) -> BindingKind {
        match namespace {
            Namespace::Value => BindingKind::Global(module),
            Namespace::Type => BindingKind::Struct(module),
        }
    }
}
