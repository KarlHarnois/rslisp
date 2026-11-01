use super::Expander;
use super::context::DefinitionContext;
use super::problems::ExpandProblems;
use super::rest_of_body::RestOfBody;
use super::shape::Shape;
use crate::core::{BodyItem, DefinitionForm, Expr, KernelForm};
use crate::diagnostic::Diagnostic;
use crate::reader::Form;

impl Expander<'_> {
    pub(super) const DEF_FORM: KernelForm = KernelForm::Definition(DefinitionForm::Def);

    pub(super) fn lower_misplaced_def(
        &mut self,
        form: &Form,
        rest_of_body: &mut RestOfBody,
        context: &DefinitionContext,
    ) -> Result<BodyItem, Diagnostic> {
        let misplaced = ExpandProblems::module_level_only(Self::DEF_FORM, form.span());
        let Ok(items) = Shape::new(Self::DEF_FORM, form).exactly(3) else {
            return Err(misplaced);
        };
        self.keep_recovered(misplaced)?;
        let scope = self.fresh_local_scope();
        rest_of_body.add_scope(scope);
        let binder = self.lower_binder(&Self::scoped(&items[1], scope), Some(context))?;
        let value = self.lower_misplaced_value(&Self::scoped(&items[2], scope))?;
        Ok(BodyItem::Let(binder, value))
    }

    fn lower_misplaced_value(&mut self, value: &Form) -> Result<Expr, Diagnostic> {
        if self.is_kernel_form(value, KernelForm::Macro) {
            return Ok(Self::placeholder(value.span()));
        }
        self.lower_recovered(value)
    }
}
