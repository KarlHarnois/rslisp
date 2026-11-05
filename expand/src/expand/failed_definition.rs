use super::Expander;
use super::context::DefinitionContext;
use crate::core::BindingId;
use crate::diagnostic::Diagnostic;
use crate::reader::Form;

impl Expander<'_> {
    pub(super) fn apply_definer(
        &mut self,
        definer: BindingId,
        call: Form,
        context: Option<&mut DefinitionContext>,
    ) -> Result<Form, Diagnostic> {
        let expansion = self.apply_macro(definer, call, context);
        self.declared_definitions.clear();
        expansion
    }
}
