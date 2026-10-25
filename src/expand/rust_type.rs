use std::rc::Rc;

use super::Expander;
use super::problems::ExpandProblems;
use super::shape::Shape;
use crate::core::{
    BindingId, CoreModule, DefinitionForm, EnumRust, Item, ItemKind, KernelForm, ParsedTemplate,
    TypeBinder, TypeExpr,
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
        let items = shape.at_least(Self::RUST_TYPE_PARTS)?;
        self.reject_old_rust_type_parameters(items)?;
        let body = vec![items[2].clone()];
        let parameterized = self.lower_head(&shape, &items[1], body)?;
        let [template_form] = parameterized.body.as_slice() else {
            return Err(shape.malformed(&items[2]));
        };
        let template = Self::type_template(&shape, template_form)?;
        Self::check_type_placeholders(
            &template,
            &parameterized.binders,
            self.bindings.get(binding).name(),
            template_form,
        )?;
        let clauses =
            self.lower_rust_type_clauses(&shape, form, items, parameterized.binders.len())?;
        core.items.push(Item {
            kind: ItemKind::RustType(binding, parameterized.binders, Rc::new(template), clauses),
            span: form.span(),
            hidden: false,
        });
        Ok(())
    }

    fn reject_old_rust_type_parameters(&self, items: &[Form]) -> Result<(), Diagnostic> {
        let body = vec![items[2].clone()];
        let Some(where_items) = self.old_parameters(&body) else {
            return Ok(());
        };
        let span = where_items[0].span();
        let kind = self
            .macro_origin_at(span)
            .map_or("rust-type", |origin| origin.name.as_ref());
        let replacement = self.old_head_replacement(kind, &items[1], where_items);
        Err(ExpandProblems::parameters_on_name(kind, &replacement, span))
    }

    fn lower_rust_type_clauses(
        &mut self,
        shape: &Shape<'_>,
        form: &Form,
        items: &[Form],
        parameters: usize,
    ) -> Result<Vec<(TypeExpr, Rc<ParsedTemplate>)>, Diagnostic> {
        if items.len() <= Self::RUST_TYPE_PARTS {
            return Ok(Vec::new());
        }
        if parameters != 1 {
            let name = shape.symbol(self.head_name(shape, &items[1])?)?;
            return Err(ExpandProblems::rust_type_clauses_need_one_parameter(
                name.name(),
                parameters,
                items[1].span(),
            ));
        }
        items[Self::RUST_TYPE_PARTS..]
            .iter()
            .map(|clause| self.lower_rust_type_clause(shape, form, clause))
            .collect()
    }

    fn lower_rust_type_clause(
        &mut self,
        shape: &Shape<'_>,
        form: &Form,
        clause: &Form,
    ) -> Result<(TypeExpr, Rc<ParsedTemplate>), Diagnostic> {
        let Some([target, template]) = clause.as_list() else {
            return Err(shape.malformed(form));
        };
        Ok((
            self.lower_type(target)?,
            Rc::new(Self::type_template(shape, template)?),
        ))
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
        let FormKind::String(text) = path.kind() else {
            return Err(Self::malformed_rust_clause(shape, clause));
        };
        if ParsedTemplate::parse(text).is_ok_and(|parsed| parsed.placeholders().next().is_some()) {
            let template = Self::type_template(shape, path)?;
            return Ok(EnumRust {
                path: Rc::clone(text),
                template: Rc::new(template),
                template_clause: true,
            });
        }
        let template = Self::enum_rust_template(text, parameters)
            .map_err(|_| Self::malformed_rust_clause(shape, clause))?;
        Ok(EnumRust {
            path: Rc::clone(text),
            template: Rc::new(template),
            template_clause: false,
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
        if let Some(name) = template.binder_names().first().copied() {
            return Err(ExpandProblems::type_placeholder_in_type_template(
                &format!("!{name}"),
                template_form.span(),
            ));
        }
        if let Some(name) = template.type_placeholders().next() {
            return Err(ExpandProblems::type_placeholder_in_type_template(
                name,
                template_form.span(),
            ));
        }
        Ok(template)
    }

    pub(super) fn check_type_placeholders(
        template: &ParsedTemplate,
        binders: &[TypeBinder],
        name: &str,
        template_form: &Form,
    ) -> Result<(), Diagnostic> {
        if let Some(index) = template
            .placeholders()
            .find(|index| *index >= binders.len())
        {
            let placeholders = template
                .placeholders()
                .max()
                .map_or(0, |placeholder| placeholder + 1);
            return Err(ExpandProblems::placeholder_without_parameter(
                index,
                binders.len(),
                name,
                template.text(),
                placeholders,
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
