use std::rc::Rc;

use super::Expander;
use super::problems::ExpandProblems;
use super::shape::Shape;
use super::type_parameters::Parameterized;
use crate::core::{
    BindingId, CoreModule, DefinitionForm, Item, ItemKind, KernelForm, ParsedTemplate, TypeBinder,
};
use crate::diagnostic::Diagnostic;
use crate::reader::Form;

impl Expander<'_> {
    const RUST_TYPE_PARTS: usize = 3;

    pub(super) fn lower_rust_type(
        &mut self,
        binding: BindingId,
        form: &Form,
        core: &mut CoreModule,
    ) -> Result<(), Diagnostic> {
        let shape = Shape::new(KernelForm::Definition(DefinitionForm::RustType), form);
        let body = &shape.exactly(Self::RUST_TYPE_PARTS)?[2..];
        let parameterized = match self.parameterized_body(body) {
            Some(where_items) => {
                self.lower_parameters(where_items, || shape.malformed(&body[0]))?
            }
            None => Parameterized {
                binders: Vec::new(),
                body: body.to_vec(),
            },
        };
        let [template_form] = parameterized.body.as_slice() else {
            return Err(shape.malformed(&body[0]));
        };
        let template = Self::type_template(&shape, template_form)?;
        Self::check_type_placeholders(&template, &parameterized.binders, template_form)?;
        core.items.push(Item {
            kind: ItemKind::RustType(binding, parameterized.binders, Rc::new(template)),
            span: form.span(),
        });
        Ok(())
    }

    fn type_template(shape: &Shape, template_form: &Form) -> Result<ParsedTemplate, Diagnostic> {
        let template = ParsedTemplate::parse(shape.string(template_form)?)
            .map_err(|_| ExpandProblems::malformed_type_template(template_form.span()))?;
        if template.borrows_any() {
            return Err(ExpandProblems::borrowed_type_placeholder(
                template_form.span(),
            ));
        }
        Ok(template)
    }

    fn check_type_placeholders(
        template: &ParsedTemplate,
        binders: &[TypeBinder],
        template_form: &Form,
    ) -> Result<(), Diagnostic> {
        if let Some(index) = template
            .placeholders()
            .find(|index| *index >= binders.len())
        {
            return Err(ExpandProblems::placeholder_without_parameter(
                index,
                binders.len(),
                template_form.span(),
            ));
        }
        let unused = binders.iter().enumerate().find(|(index, _)| {
            !template
                .placeholders()
                .any(|placeholder| placeholder == *index)
        });
        match unused {
            Some((index, binder)) => {
                Err(ExpandProblems::unused_type_placeholder(index, binder.span))
            }
            None => Ok(()),
        }
    }
}
