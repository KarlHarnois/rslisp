use std::collections::VecDeque;
use std::rc::Rc;

use super::Expander;
use super::context::DefinitionContext;
use super::problems::ExpandProblems;
use super::shape::Shape;
use crate::core::{
    Binder, BindingKind, Block, BodyItem, Expr, ExprKind, FieldLabel, KernelForm, Lambda, Literal,
    Namespace, RustTemplate,
};
use crate::diagnostic::Diagnostic;
use crate::reader::{Form, FormKind, Symbol};
use crate::source::Span;

impl Expander<'_> {
    const RUST_FORM_WITH_TWIN: usize = 4;

    pub(super) fn lower_expr(&mut self, form: &Form) -> Result<Expr, Diagnostic> {
        let span = form.span();
        let kind = match form.kind() {
            FormKind::Symbol(symbol) => self.lower_reference(symbol, span)?,
            FormKind::String(text) => ExprKind::Literal(Literal::String(Rc::clone(text))),
            FormKind::Number(number) => ExprKind::Literal(Literal::Number(number.clone())),
            FormKind::List(items) => return self.lower_list(form, items),
        };
        Ok(Expr::new(kind, span))
    }

    pub(super) fn is_kernel_form(&self, form: &Form, kernel: KernelForm) -> bool {
        self.kernel_head(form) == Some(kernel)
    }

    pub(super) fn lower_lambda(
        &mut self,
        shape: &Shape,
        params: &Form,
        body: &[Form],
    ) -> Result<Lambda, Diagnostic> {
        let scope = self.scopes.fresh();
        let mut params = shape.list(params)?.to_vec();
        let mut body = body.to_vec();
        params
            .iter_mut()
            .chain(body.iter_mut())
            .for_each(|form| form.add_scope(scope));
        let params = params
            .iter()
            .map(|param| self.lower_binder(param, None))
            .collect::<Result<Vec<Binder>, Diagnostic>>()?;
        let body = self.lower_body(&body)?;
        Ok(Lambda { params, body })
    }

    pub(super) fn field_label(&self, shape: &Shape, form: &Form) -> Result<FieldLabel, Diagnostic> {
        Ok(FieldLabel {
            name: shape.symbol(form)?.shared_name(),
            span: form.span(),
        })
    }

    fn kernel_head(&self, form: &Form) -> Option<KernelForm> {
        let head = form.as_list()?.first()?.as_symbol()?;
        match self.kind(self.try_resolve(head, Namespace::Value)?) {
            BindingKind::KernelForm(kernel) => Some(kernel),
            _ => None,
        }
    }

    fn lower_reference(&self, symbol: &Symbol, span: Span) -> Result<ExprKind, Diagnostic> {
        let binding = self.resolve(symbol, Namespace::Value, span)?;
        match self.kind(binding) {
            BindingKind::KernelForm(kernel) => Err(ExpandProblems::form_as_value(kernel, span)),
            BindingKind::Macro => Err(ExpandProblems::macro_as_value(symbol.name(), span)),
            _ => Ok(ExprKind::Var(binding)),
        }
    }

    fn lower_list(&mut self, form: &Form, items: &[Form]) -> Result<Expr, Diagnostic> {
        let Some(head) = items.first() else {
            return Err(ExpandProblems::empty_call(form.span()));
        };
        if let Some(kernel) = self.kernel_head(form) {
            return self.lower_kernel(kernel, form);
        }
        if let Some(binding) = self.macro_head(form) {
            let expanded = self.apply_macro(binding, form.clone(), None)?;
            return self.lower_expr(&expanded);
        }
        let callee = self.lower_expr(head)?;
        let arguments = items[1..]
            .iter()
            .map(|argument| self.lower_expr(argument))
            .collect::<Result<Vec<Expr>, Diagnostic>>()?;
        Ok(Expr::new(
            ExprKind::Call(Box::new(callee), arguments),
            form.span(),
        ))
    }

    fn lower_kernel(&mut self, kernel: KernelForm, form: &Form) -> Result<Expr, Diagnostic> {
        let shape = Shape::new(kernel, form);
        let span = form.span();
        let kind = match kernel {
            KernelForm::Fn => {
                let items = shape.at_least(2)?;
                ExprKind::Fn(Rc::new(self.lower_lambda(
                    &shape,
                    &items[1],
                    &items[2..],
                )?))
            }
            KernelForm::Do => ExprKind::Block(self.lower_body(&shape.items()[1..])?),
            KernelForm::If => self.lower_if(&shape)?,
            KernelForm::Quote => ExprKind::Quote(shape.exactly(2)?[1].clone()),
            KernelForm::New => self.lower_new(&shape)?,
            KernelForm::Field => self.lower_field_access(&shape)?,
            KernelForm::Rust => self.lower_rust(&shape)?,
            KernelForm::Let => return Err(ExpandProblems::let_outside_body(span)),
            KernelForm::Macro => return Err(ExpandProblems::macro_outside_def(span)),
            KernelForm::Def
            | KernelForm::Type
            | KernelForm::Struct
            | KernelForm::RustTrait
            | KernelForm::Use
            | KernelForm::Export => {
                return Err(ExpandProblems::module_level_only(kernel, span));
            }
        };
        Ok(Expr::new(kind, span))
    }

    fn lower_body(&mut self, forms: &[Form]) -> Result<Block, Diagnostic> {
        let mut context = DefinitionContext::default();
        let mut queue: VecDeque<Form> = forms.iter().cloned().collect();
        let mut items = Vec::new();
        while let Some(form) = queue.pop_front() {
            let form = self.expand_head(form, Some(&mut context))?;
            let item = if self.is_kernel_form(&form, KernelForm::Let) {
                self.lower_let(&form, &mut queue, &context)?
            } else {
                BodyItem::Expr(self.lower_expr(&form)?)
            };
            items.push(item);
        }
        Ok(Block { items })
    }

    fn lower_let(
        &mut self,
        form: &Form,
        rest_of_body: &mut VecDeque<Form>,
        context: &DefinitionContext,
    ) -> Result<BodyItem, Diagnostic> {
        let items = Shape::new(KernelForm::Let, form).exactly(3)?;
        let value = self.lower_expr(&items[2])?;
        let scope = self.scopes.fresh();
        let mut binder = items[1].clone();
        binder.add_scope(scope);
        rest_of_body
            .iter_mut()
            .for_each(|later| later.add_scope(scope));
        Ok(BodyItem::Let(
            self.lower_binder(&binder, Some(context))?,
            value,
        ))
    }

    fn lower_binder(
        &mut self,
        form: &Form,
        context: Option<&DefinitionContext>,
    ) -> Result<Binder, Diagnostic> {
        let (name, annotation) = Self::split_binder(form);
        let annotation = annotation
            .map(|annotation| self.lower_type(annotation))
            .transpose()?;
        let binding = self.define_local(name, Namespace::Value, BindingKind::Local, context)?;
        Ok(Binder {
            binding,
            annotation,
            span: form.span(),
        })
    }

    fn lower_if(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let items = shape.exactly(4)?;
        let test = self.lower_expr(&items[1])?;
        let then = self.lower_expr(&items[2])?;
        let otherwise = self.lower_expr(&items[3])?;
        Ok(ExprKind::If(
            Box::new(test),
            Box::new(then),
            Box::new(otherwise),
        ))
    }

    fn lower_new(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let items = shape.at_least(2)?;
        let name = shape.symbol(&items[1])?;
        let binding = self.resolve(name, Namespace::Type, items[1].span())?;
        if !matches!(self.kind(binding), BindingKind::Struct(_)) {
            return Err(ExpandProblems::wrong_namespace(
                name.name(),
                "a struct",
                items[1].span(),
            ));
        }
        let fields = items[2..]
            .iter()
            .map(|field| self.lower_field_value(shape, field))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        Ok(ExprKind::New(binding, fields))
    }

    fn lower_field_value(
        &mut self,
        shape: &Shape,
        field: &Form,
    ) -> Result<(FieldLabel, Expr), Diagnostic> {
        let [label, value] = shape.list(field)? else {
            return Err(shape.malformed(field));
        };
        Ok((self.field_label(shape, label)?, self.lower_expr(value)?))
    }

    fn lower_field_access(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let items = shape.exactly(3)?;
        let value = self.lower_expr(&items[1])?;
        Ok(ExprKind::Field(
            Box::new(value),
            self.field_label(shape, &items[2])?,
        ))
    }

    fn lower_rust(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let items = shape.at_least(3)?;
        if items.len() > Self::RUST_FORM_WITH_TWIN {
            return Err(shape.malformed(&items[Self::RUST_FORM_WITH_TWIN]));
        }
        let template = shape.string(&items[1])?.into();
        let type_expr = self.lower_type(&items[2])?;
        let twin = items.get(3).map(|twin| self.lower_expr(twin)).transpose()?;
        Ok(ExprKind::Rust(Rc::new(RustTemplate {
            template,
            type_expr,
            twin,
        })))
    }
}
