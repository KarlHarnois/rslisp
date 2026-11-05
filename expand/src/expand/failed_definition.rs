use super::Expander;
use super::context::DefinitionContext;
use crate::core::{BindingId, BindingKind, Expr, ExprKind, Leaf, ModuleId, Namespace};
use crate::diagnostic::Diagnostic;
use crate::reader::Form;

impl Expander<'_> {
    pub(super) fn apply_definer(
        &mut self,
        definer: BindingId,
        call: Form,
        context: Option<&mut DefinitionContext>,
    ) -> Result<Form, Diagnostic> {
        let context = context.expect("a top-level call expands in a definition context");
        let expansion = self.apply_macro(definer, call, Some(context));
        let declared = std::mem::take(&mut self.declared_definitions);
        expansion.inspect_err(|_| self.bind_failed_names(&declared, context))
    }

    pub(super) fn failed_use(&self, form: &Form) -> Option<Expr> {
        if self.failed_definitions.is_empty() {
            return None;
        }
        let head = form.as_list()?.first()?.as_symbol()?;
        let binding = self.try_resolve(head, Namespace::Value)?;
        self.failed_definitions
            .contains(&binding)
            .then(|| Expr::new(ExprKind::Leaf(Leaf::Var(binding)), form.span()))
    }

    fn bind_failed_names(&mut self, declared: &[(Form, Namespace)], context: &DefinitionContext) {
        let module = self
            .current_module
            .expect("a top-level call expands inside a module");
        for (name, namespace) in declared {
            self.bind_failed_name(name, *namespace, module, context);
        }
    }

    fn bind_failed_name(
        &mut self,
        name: &Form,
        namespace: Namespace,
        module: ModuleId,
        context: &DefinitionContext,
    ) {
        let kind = Self::failed_kind(namespace, module);
        if let Ok(binding) = self.define_unrecorded_global(name, namespace, kind, context) {
            self.failed_definitions.insert(binding);
            self.tables.globals.mark_failed(binding);
        }
    }

    fn failed_kind(namespace: Namespace, module: ModuleId) -> BindingKind {
        match namespace {
            Namespace::Value => BindingKind::Global(module),
            Namespace::Type => BindingKind::Struct(module),
        }
    }
}
