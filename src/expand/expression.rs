use std::collections::VecDeque;
use std::rc::Rc;

use super::Expander;
use super::context::DefinitionContext;
use super::problems::ExpandProblems;
use super::recur_check::RecurCheck;
use super::shape::Shape;
use crate::core::{
    Binder, BindingKind, Block, BodyItem, Control, ControlForm, Expr, ExprKind, ExpressionForm,
    FieldLabel, KernelForm, Lambda, Leaf, LeafForm, Literal, Namespace, ParsedTemplate,
    RustTemplate, StructExpr, StructForm,
};
use crate::diagnostic::Diagnostic;
use crate::reader::{Form, FormKind, Symbol};
use crate::scope::Phase;
use crate::source::Span;
use crate::stack::DeepRecursion;

impl Expander<'_> {
    pub(super) fn lower_expr(&mut self, form: &Form) -> Result<Expr, Diagnostic> {
        DeepRecursion::guard(|| self.lower_form(form))
    }

    fn lower_form(&mut self, form: &Form) -> Result<Expr, Diagnostic> {
        let span = form.span();
        let kind = match form.kind() {
            FormKind::Symbol(symbol) => self.lower_reference(symbol, span)?,
            FormKind::String(text) => {
                ExprKind::Leaf(Leaf::Literal(Literal::String(Rc::clone(text))))
            }
            FormKind::Number(number) => {
                ExprKind::Leaf(Leaf::Literal(Literal::Number(number.clone())))
            }
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
        let scope = self.fresh_local_scope();
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
        RecurCheck::inside(params.len()).block(&body)?;
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
            BindingKind::Boolean(value) => Ok(ExprKind::Leaf(Leaf::Literal(Literal::Bool(value)))),
            _ => Ok(ExprKind::Leaf(Leaf::Var(binding))),
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
            return self.lower_expansion(&expanded);
        }
        let callee = self.lower_recovered(head)?;
        let arguments = items[1..]
            .iter()
            .map(|argument| self.lower_recovered(argument))
            .collect::<Result<Vec<Expr>, Diagnostic>>()?;
        Ok(Expr::new(
            ExprKind::Call(Box::new(callee), arguments),
            form.span(),
        ))
    }

    fn lower_recovered(&mut self, form: &Form) -> Result<Expr, Diagnostic> {
        match self.lower_expr(form) {
            Ok(expr) => Ok(expr),
            Err(diagnostic) => self.recover(diagnostic, form.span()),
        }
    }

    fn recover(&mut self, diagnostic: Diagnostic, span: Span) -> Result<Expr, Diagnostic> {
        if self.phase == Phase::CompileTime {
            return Err(diagnostic);
        }
        self.recovered_diagnostics.push(diagnostic);
        Ok(Expr::new(
            ExprKind::Leaf(Leaf::Literal(Literal::Bool(false))),
            span,
        ))
    }

    fn lower_kernel(&mut self, kernel: KernelForm, form: &Form) -> Result<Expr, Diagnostic> {
        let span = form.span();
        let kind = match kernel {
            KernelForm::Expression(expression) => {
                self.lower_expression_form(expression, &Shape::new(kernel, form))
            }
            KernelForm::Let => Err(ExpandProblems::let_outside_body(span)),
            KernelForm::Macro => Err(ExpandProblems::macro_outside_def(span)),
            KernelForm::Definition(_) => Err(ExpandProblems::module_level_only(kernel, span)),
        };
        kind.map(|kind| Expr::new(kind, span))
    }

    fn lower_expression_form(
        &mut self,
        expression: ExpressionForm,
        shape: &Shape,
    ) -> Result<ExprKind, Diagnostic> {
        match expression {
            ExpressionForm::Fn => self.lower_fn(shape),
            ExpressionForm::Recur => self.lower_recur(shape),
            ExpressionForm::Leaf(form) => self.lower_leaf_form(form, shape),
            ExpressionForm::Control(form) => self.lower_control_form(form, shape),
            ExpressionForm::Struct(form) => self.lower_struct_form(form, shape),
        }
    }

    fn lower_leaf_form(&mut self, form: LeafForm, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        match form {
            LeafForm::Quote => self.lower_quote(shape),
            LeafForm::Rust => self.lower_rust(shape),
        }
    }

    fn lower_control_form(
        &mut self,
        form: ControlForm,
        shape: &Shape,
    ) -> Result<ExprKind, Diagnostic> {
        match form {
            ControlForm::Do => self.lower_do(shape),
            ControlForm::If => self.lower_if(shape),
        }
    }

    fn lower_struct_form(
        &mut self,
        form: StructForm,
        shape: &Shape,
    ) -> Result<ExprKind, Diagnostic> {
        match form {
            StructForm::New => self.lower_new(shape),
            StructForm::Field => self.lower_field_access(shape),
        }
    }

    fn lower_fn(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let items = shape.at_least(2)?;
        let lambda = self.lower_lambda(shape, &items[1], &items[2..])?;
        Ok(ExprKind::Fn(Rc::new(lambda)))
    }

    fn lower_do(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let body = self.lower_body(&shape.items()[1..])?;
        Ok(ExprKind::Control(Control::Block(body)))
    }

    fn lower_quote(&self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let quoted = self.quoted(&shape.exactly(2)?[1]);
        Ok(ExprKind::Leaf(Leaf::Quote(quoted)))
    }

    fn lower_body(&mut self, forms: &[Form]) -> Result<Block, Diagnostic> {
        let mut context = DefinitionContext::default();
        let mut queue: VecDeque<Form> = forms.iter().cloned().collect();
        let mut items = Vec::new();
        while let Some(form) = queue.pop_front() {
            items.push(self.lower_body_item(form, &mut queue, &mut context)?);
        }
        Ok(Block { items })
    }

    fn lower_body_item(
        &mut self,
        form: Form,
        queue: &mut VecDeque<Form>,
        context: &mut DefinitionContext,
    ) -> Result<BodyItem, Diagnostic> {
        let span = form.span();
        match self.expand_head(form, Some(context)) {
            Ok(form) if self.is_kernel_form(&form, KernelForm::Let) => {
                self.lower_let_or_recover(&form, queue, context, span)
            }
            Ok(form) => Ok(BodyItem::Expr(self.lower_recovered(&form)?)),
            Err(diagnostic) => Ok(BodyItem::Expr(self.recover(diagnostic, span)?)),
        }
    }

    fn lower_let_or_recover(
        &mut self,
        form: &Form,
        queue: &mut VecDeque<Form>,
        context: &DefinitionContext,
        span: Span,
    ) -> Result<BodyItem, Diagnostic> {
        match self.lower_let(form, queue, context) {
            Ok(item) => Ok(item),
            Err(diagnostic) => Ok(BodyItem::Expr(self.recover(diagnostic, span)?)),
        }
    }

    fn lower_let(
        &mut self,
        form: &Form,
        rest_of_body: &mut VecDeque<Form>,
        context: &DefinitionContext,
    ) -> Result<BodyItem, Diagnostic> {
        let items = Shape::new(KernelForm::Let, form).exactly(3)?;
        let value = self.lower_recovered(&items[2])?;
        let scope = self.fresh_local_scope();
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
        let test = self.lower_recovered(&items[1])?;
        let then = self.lower_recovered(&items[2])?;
        let otherwise = self.lower_recovered(&items[3])?;
        Ok(ExprKind::Control(Control::If(
            Box::new(test),
            Box::new(then),
            Box::new(otherwise),
        )))
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
        Ok(ExprKind::Struct(StructExpr::New(binding, fields)))
    }

    fn lower_field_value(
        &mut self,
        shape: &Shape,
        field: &Form,
    ) -> Result<(FieldLabel, Expr), Diagnostic> {
        let [label, value] = shape.list(field)? else {
            return Err(shape.malformed(field));
        };
        Ok((
            self.field_label(shape, label)?,
            self.lower_recovered(value)?,
        ))
    }

    fn lower_field_access(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let items = shape.exactly(3)?;
        let value = self.lower_recovered(&items[1])?;
        Ok(ExprKind::Struct(StructExpr::Field(
            Box::new(value),
            self.field_label(shape, &items[2])?,
        )))
    }

    fn lower_rust(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let items = shape.exactly(3)?;
        let text = shape.string(&items[1])?;
        ParsedTemplate::parse(text)
            .map_err(|_| ExpandProblems::malformed_template(items[1].span()))?;
        let template = text.into();
        let type_expr = self.lower_type(&items[2])?;
        Ok(ExprKind::Leaf(Leaf::Rust(Rc::new(RustTemplate {
            template,
            type_expr,
        }))))
    }

    fn lower_recur(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let arguments = shape.items()[1..]
            .iter()
            .map(|argument| self.lower_recovered(argument))
            .collect::<Result<Vec<Expr>, Diagnostic>>()?;
        Ok(ExprKind::Recur(arguments))
    }
}
