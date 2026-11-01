use super::Expander;
use super::context::DefinitionContext;
use super::problems::ExpandProblems;
use super::rest_of_body::RestOfBody;
use super::shape::Shape;
use crate::core::{
    BindingKind, Block, BodyItem, Control, ControlForm, DefinitionForm, Expr, ExprKind,
    ExpressionForm, KernelForm, Namespace,
};
use crate::diagnostic::Diagnostic;
use crate::reader::Form;

impl Expander<'_> {
    pub(super) const DEF_FORM: KernelForm = KernelForm::Definition(DefinitionForm::Def);
    pub(super) const STRUCT_FORM: KernelForm = KernelForm::Definition(DefinitionForm::Struct);
    pub(super) const DO_FORM: KernelForm =
        KernelForm::Expression(ExpressionForm::Control(ControlForm::Do));

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

    pub(super) fn lower_misplaced_struct(
        &mut self,
        form: &Form,
        rest_of_body: &mut RestOfBody,
        context: &DefinitionContext,
    ) -> Result<BodyItem, Diagnostic> {
        let misplaced = ExpandProblems::module_level_only(Self::STRUCT_FORM, form.span());
        let shape = Shape::new(Self::STRUCT_FORM, form);
        let Some(name) = shape
            .at_least(2)
            .ok()
            .and_then(|items| self.head_name(&shape, &items[1]).ok())
        else {
            return Err(misplaced);
        };
        self.keep_recovered(misplaced)?;
        let scope = self.fresh_local_scope();
        rest_of_body.add_scope(scope);
        let module = self
            .current_module
            .expect("a body expands while a module expands");
        let scoped_name = Self::scoped(name, scope);
        self.define_local(
            &scoped_name,
            Namespace::Type,
            BindingKind::Struct(module),
            Some(context),
        )?;
        Ok(BodyItem::Expr(Self::placeholder(form.span())))
    }

    pub(super) fn holds_definition(&self, form: &Form) -> bool {
        form.as_list().unwrap_or_default()[1..]
            .iter()
            .any(|item| matches!(self.kernel_head(item), Some(KernelForm::Definition(_))))
    }

    pub(super) fn lower_spliced_do(
        &mut self,
        form: &Form,
        rest_of_body: &mut RestOfBody,
        context: &mut DefinitionContext,
    ) -> Result<BodyItem, Diagnostic> {
        let forms = &form.as_list().unwrap_or_default()[1..];
        let items = self.lower_body_items(&mut rest_of_body.splice(forms), context)?;
        let block = ExprKind::Control(Control::Block(Block { items }));
        Ok(BodyItem::Expr(Expr::new(block, form.span())))
    }
}
