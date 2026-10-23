use std::rc::Rc;

use super::Expander;
use super::problems::ExpandProblems;
use super::shape::Shape;
use crate::core::{
    BindingId, CoreModule, DefinitionForm, EnumRust, Item, ItemKind, KernelForm, ParsedTemplate,
    TypeBinder,
};
use crate::diagnostic::Diagnostic;
use crate::reader::{Form, FormKind};

impl Expander<'_> {
    const RUST_TYPE_PARTS: usize = 3;
    const RUST_CLAUSE: &'static str = "rust";
    const RUST_CLAUSE_HELP: &'static str = "write it as `(rust \"Path\")`";

    pub(super) fn lower_rust_type(
        &mut self,
        binding: BindingId,
        form: &Form,
        core: &mut CoreModule,
    ) -> Result<(), Diagnostic> {
        let shape = Shape::new(KernelForm::Definition(DefinitionForm::RustType), form);
        let items = shape.exactly(Self::RUST_TYPE_PARTS)?;
        let body = vec![items[2].clone()];
        if let Some(where_items) = self.old_parameters(&body) {
            let span = where_items[0].span();
            let kind = self
                .macro_origin_at(span)
                .map_or("rust-type", |origin| origin.name.as_ref());
            let replacement = self.old_head_replacement(kind, &items[1], where_items);
            return Err(ExpandProblems::parameters_on_name(kind, &replacement, span));
        }
        let parameterized = self.lower_head(&shape, &items[1], body)?;
        let [template_form] = parameterized.body.as_slice() else {
            return Err(shape.malformed(&items[2]));
        };
        let template = Self::type_template(&shape, template_form)?;
        Self::check_type_placeholders(&template, &parameterized.binders, template_form)?;
        core.items.push(Item {
            kind: ItemKind::RustType(binding, parameterized.binders, Rc::new(template)),
            span: form.span(),
        });
        Ok(())
    }

    pub(super) fn starts_rust_clause(form: &Form) -> bool {
        form.as_list()
            .and_then(<[Form]>::first)
            .and_then(Form::as_symbol)
            .is_some_and(|symbol| symbol.name() == Self::RUST_CLAUSE)
    }

    pub(super) fn enum_rust(
        shape: &Shape,
        clause: &Form,
        parameters: usize,
    ) -> Result<EnumRust, Diagnostic> {
        let Some([_, path]) = clause.as_list() else {
            return Err(Self::malformed_rust_clause(shape, clause));
        };
        let FormKind::String(path) = path.kind() else {
            return Err(Self::malformed_rust_clause(shape, clause));
        };
        let template = Self::enum_rust_template(path, parameters)
            .map_err(|_| Self::malformed_rust_clause(shape, clause))?;
        Ok(EnumRust {
            path: Rc::clone(path),
            template: Rc::new(template),
        })
    }

    fn enum_rust_template(
        path: &str,
        parameters: usize,
    ) -> Result<ParsedTemplate, crate::core::MalformedPlaceholder> {
        let arguments: Vec<String> = (0..parameters)
            .map(|index| format!("{{{index}}}"))
            .collect();
        let template = if arguments.is_empty() {
            path.to_string()
        } else {
            format!("{path}<{}>", arguments.join(", "))
        };
        ParsedTemplate::parse(&template)
    }

    fn malformed_rust_clause(shape: &Shape, clause: &Form) -> Diagnostic {
        shape.malformed_with_help(clause, Self::RUST_CLAUSE_HELP)
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
