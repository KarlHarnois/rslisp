use std::collections::VecDeque;
use std::rc::Rc;

use super::Expander;
use super::context::DefinitionContext;
use super::problems::ExpandProblems;
use super::recur_check::RecurCheck;
use super::shape::Shape;
use super::try_check::TryCheck;
use crate::core::{
    Arm, Binder, BindingId, BindingKind, Block, BodyItem, Control, ControlForm, Expr, ExprKind,
    ExpressionForm, FieldLabel, KernelForm, Lambda, Leaf, LeafForm, Literal, MapOperation,
    Namespace, ParsedTemplate, Pattern, PatternKind, ReferenceKind, RustTemplate, StructExpr,
    StructForm, TypeExpr, TypeExprKind, VariantInfo,
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
            FormKind::Symbol(symbol) => return self.lower_symbol(symbol, form),
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
        TryCheck::inside().block(&body)?;
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

    fn lower_symbol(&mut self, symbol: &Symbol, form: &Form) -> Result<Expr, Diagnostic> {
        let span = form.span();
        let binding = self.resolve(symbol, Namespace::Value, span)?;
        if matches!(self.kind(binding), BindingKind::Macro(_)) {
            return self.lower_macro_value(binding, symbol, form);
        }
        Ok(Expr::new(self.lower_reference(binding, span)?, span))
    }

    fn lower_macro_value(
        &mut self,
        binding: BindingId,
        symbol: &Symbol,
        form: &Form,
    ) -> Result<Expr, Diagnostic> {
        match self.value_expansion(binding, form.clone()) {
            Some(expanded) => self.lower_expansion(&expanded),
            None => Err(self.macro_as_value(binding, symbol, form.span())),
        }
    }

    fn macro_as_value(&mut self, binding: BindingId, symbol: &Symbol, span: Span) -> Diagnostic {
        ExpandProblems::macro_as_value(
            symbol.name(),
            span,
            self.macro_function(binding, symbol, span)
                .map(|function| Rc::from(self.bindings.get(function).name())),
        )
    }

    fn lower_reference(&mut self, binding: BindingId, span: Span) -> Result<ExprKind, Diagnostic> {
        match self.kind(binding) {
            BindingKind::KernelForm(kernel) => Err(ExpandProblems::form_as_value(kernel, span)),
            BindingKind::Boolean(value) => Ok(ExprKind::Leaf(Leaf::Literal(Literal::Bool(value)))),
            BindingKind::Variant(_, info) if info.arity == 0 => {
                Ok(ExprKind::Variant(binding, Vec::new()))
            }
            BindingKind::Variant(_, info) => Err(ExpandProblems::variant_as_value(
                self.bindings.get(binding).name(),
                info.arity,
                span,
            )),
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
        if let Some(binding) = self.variant_head(form) {
            let arguments = items[1..]
                .iter()
                .map(|argument| self.lower_recovered(argument))
                .collect::<Result<Vec<Expr>, Diagnostic>>()?;
            return Ok(Expr::new(
                ExprKind::Variant(binding, arguments),
                form.span(),
            ));
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

    fn variant_head(&self, form: &Form) -> Option<BindingId> {
        let head = form.as_list()?.first()?.as_symbol()?;
        let binding = self.try_resolve(head, Namespace::Value)?;
        matches!(
            self.kind(binding),
            BindingKind::Variant(_, VariantInfo { arity, .. }) if arity > 0
        )
        .then_some(binding)
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
            ExpressionForm::Try => self.lower_try(shape),
            other => self.lower_value_form(other, shape),
        }
    }

    fn lower_value_form(
        &mut self,
        expression: ExpressionForm,
        shape: &Shape,
    ) -> Result<ExprKind, Diagnostic> {
        match expression {
            ExpressionForm::Leaf(form) => self.lower_leaf_form(form, shape),
            ExpressionForm::Borrow(kind) => self.lower_borrow(kind, shape),
            other => self.lower_other_expression(other, shape),
        }
    }

    fn lower_other_expression(
        &mut self,
        expression: ExpressionForm,
        shape: &Shape,
    ) -> Result<ExprKind, Diagnostic> {
        match expression {
            ExpressionForm::MapAt => self.lower_map_at(shape),
            ExpressionForm::SetAt => self.lower_set_at(shape),
            other => self.lower_simple_expression(other, shape),
        }
    }

    fn lower_simple_expression(
        &mut self,
        expression: ExpressionForm,
        shape: &Shape,
    ) -> Result<ExprKind, Diagnostic> {
        match expression {
            ExpressionForm::Deref => self.lower_deref(shape),
            ExpressionForm::Control(form) => self.lower_control_form(form, shape),
            ExpressionForm::Struct(form) => self.lower_struct_form(form, shape),
            ExpressionForm::Match => self.lower_match(shape),
            ExpressionForm::Assoc => self.lower_assoc(shape),
            _ => unreachable!("the caller matches a simple expression form"),
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

    fn lower_borrow(&mut self, kind: ReferenceKind, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let items = shape.exactly(2)?;
        let value = self.lower_expr(&items[1])?;
        Ok(ExprKind::Borrow(kind, Box::new(value)))
    }

    fn lower_deref(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let items = shape.exactly(2)?;
        let value = self.lower_expr(&items[1])?;
        Ok(ExprKind::Deref(Box::new(value)))
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
        if shape.items().len() == 3 {
            return Err(shape.malformed_form_with_help(
                "write `(when test body ...)` to run forms only when the test holds",
            ));
        }
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
        if items[1]
            .as_symbol()
            .is_some_and(|symbol| symbol.name() == "_")
        {
            return self.lower_new_update(shape);
        }
        self.lower_new_construction(shape)
    }

    fn lower_new_update(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let items = shape.at_least(3)?;
        let base = self.lower_expr(&items[2])?;
        let fields = self.lower_fields(shape, &items[3..])?;
        Ok(ExprKind::Struct(StructExpr::Update(Box::new(base), fields)))
    }

    fn lower_new_construction(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
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
        let fields = self.lower_fields(shape, &items[2..])?;
        Ok(ExprKind::Struct(StructExpr::New(binding, fields)))
    }

    fn lower_fields(
        &mut self,
        shape: &Shape,
        fields: &[Form],
    ) -> Result<Vec<(FieldLabel, Expr)>, Diagnostic> {
        fields
            .iter()
            .map(|field| self.lower_field_value(shape, field))
            .collect()
    }

    fn lower_assoc(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let items = shape.exactly(4)?;
        let value = self.lower_expr(&items[1])?;
        let keys = shape
            .list(&items[2])?
            .iter()
            .map(|key| self.lower_expr(key))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        let item = self.lower_expr(&items[3])?;
        Ok(ExprKind::Assoc(Box::new(value), keys, Box::new(item)))
    }

    fn lower_map_at(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let items = shape.at_least(3)?;
        let operation = items[1]
            .as_symbol()
            .and_then(|symbol| MapOperation::of(symbol.name()))
            .ok_or_else(|| shape.malformed(&items[1]))?;
        let value = self.lower_expr(&items[2])?;
        let arguments = items[3..]
            .iter()
            .map(|argument| self.lower_expr(argument))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        Ok(ExprKind::MapAt(operation, Box::new(value), arguments))
    }

    fn lower_set_at(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let items = shape.exactly(4)?;
        let value = self.lower_expr(&items[1])?;
        let keys = shape
            .list(&items[2])?
            .iter()
            .map(|key| self.lower_expr(key))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        let item = self.lower_expr(&items[3])?;
        Ok(ExprKind::SetAt(Box::new(value), keys, Box::new(item)))
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

    fn lower_match(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let items = shape.at_least(3)?;
        let value = self.lower_recovered(&items[1])?;
        let arms = items[2..]
            .iter()
            .map(|arm| self.lower_arm(shape, arm))
            .collect::<Result<Vec<Arm>, Diagnostic>>()?;
        Ok(ExprKind::Control(Control::Match(Box::new(value), arms)))
    }

    fn lower_arm(&mut self, shape: &Shape, arm: &Form) -> Result<Arm, Diagnostic> {
        let items = shape.list(arm)?;
        let (pattern, body) = items
            .split_first()
            .ok_or_else(|| ExpandProblems::arm_body(arm.span()))?;
        let [body] = body else {
            return Err(ExpandProblems::arm_body(arm.span()));
        };
        let scope = self.fresh_local_scope();
        let mut pattern = pattern.clone();
        let mut body = body.clone();
        pattern.add_scope(scope);
        body.add_scope(scope);
        let pattern = self.lower_pattern(&pattern)?;
        let body = self.lower_body(std::slice::from_ref(&body))?;
        Ok(Arm {
            pattern,
            body,
            span: arm.span(),
        })
    }

    fn lower_pattern(&mut self, form: &Form) -> Result<Pattern, Diagnostic> {
        let span = form.span();
        let kind = match form.kind() {
            FormKind::Symbol(symbol) => self.lower_symbol_pattern(form, symbol)?,
            FormKind::List(items) => self.lower_list_pattern(items, span)?,
            FormKind::Number(number) => PatternKind::Literal(Literal::Number(number.clone())),
            FormKind::String(text) => PatternKind::Literal(Literal::String(Rc::clone(text))),
        };
        Ok(Pattern::new(kind, span))
    }

    fn lower_symbol_pattern(
        &mut self,
        form: &Form,
        symbol: &Symbol,
    ) -> Result<PatternKind, Diagnostic> {
        if symbol.name() == "_" {
            return Ok(PatternKind::Wildcard);
        }
        if let Some(kind) = self.named_symbol_pattern(form, symbol)? {
            return Ok(kind);
        }
        let outer = self.try_resolve(symbol, Namespace::Value);
        let candidates = self.variant_candidates(symbol, true);
        let binding = self.define_local(form, Namespace::Value, BindingKind::Local, None)?;
        if candidates.is_empty() {
            return Ok(PatternKind::Bind(binding));
        }
        Ok(PatternKind::BareName {
            name: symbol.shared_name(),
            binding,
            candidates,
            outer,
        })
    }

    fn named_symbol_pattern(
        &self,
        form: &Form,
        symbol: &Symbol,
    ) -> Result<Option<PatternKind>, Diagnostic> {
        let Some(binding) = self.try_resolve(symbol, Namespace::Value) else {
            return Ok(None);
        };
        match self.kind(binding) {
            BindingKind::Boolean(value) => Ok(Some(PatternKind::Literal(Literal::Bool(value)))),
            BindingKind::Variant(_, info) if info.arity == 0 => {
                Ok(Some(PatternKind::Variant(binding, Vec::new())))
            }
            BindingKind::Variant(_, _) => Err(ExpandProblems::not_a_pattern(form.span())),
            _ => Ok(None),
        }
    }

    fn lower_list_pattern(
        &mut self,
        items: &[Form],
        span: Span,
    ) -> Result<PatternKind, Diagnostic> {
        let Some(head) = items.first().and_then(Form::as_symbol) else {
            return Err(ExpandProblems::not_a_pattern(span));
        };
        if let Some(binding) = self.variant_pattern_head(head) {
            return self.variant_pattern(binding, &items[1..]);
        }
        if let Some(binding) = self.struct_pattern_head(head) {
            return self.struct_pattern(binding, &items[1..]);
        }
        self.short_variant_pattern(head, &items[1..], span)
    }

    fn variant_pattern_head(&self, head: &Symbol) -> Option<BindingId> {
        let binding = self.try_resolve(head, Namespace::Value)?;
        matches!(self.kind(binding), BindingKind::Variant(_, _)).then_some(binding)
    }

    fn struct_pattern_head(&self, head: &Symbol) -> Option<BindingId> {
        let binding = self.try_resolve(head, Namespace::Type)?;
        matches!(self.kind(binding), BindingKind::Struct(_)).then_some(binding)
    }

    fn variant_pattern(
        &mut self,
        binding: BindingId,
        arguments: &[Form],
    ) -> Result<PatternKind, Diagnostic> {
        Ok(PatternKind::Variant(
            binding,
            self.lower_pattern_arguments(arguments)?,
        ))
    }

    fn struct_pattern(
        &mut self,
        binding: BindingId,
        arguments: &[Form],
    ) -> Result<PatternKind, Diagnostic> {
        Ok(PatternKind::Struct(
            binding,
            self.lower_pattern_arguments(arguments)?,
        ))
    }

    fn short_variant_pattern(
        &mut self,
        head: &Symbol,
        arguments: &[Form],
        span: Span,
    ) -> Result<PatternKind, Diagnostic> {
        let candidates = self.variant_candidates(head, false);
        if candidates.is_empty() {
            return Err(ExpandProblems::not_a_pattern(span));
        }
        Ok(PatternKind::ShortVariant {
            name: head.shared_name(),
            candidates,
            arguments: self.lower_pattern_arguments(arguments)?,
        })
    }

    fn lower_pattern_arguments(&mut self, forms: &[Form]) -> Result<Vec<Pattern>, Diagnostic> {
        forms.iter().map(|form| self.lower_pattern(form)).collect()
    }

    fn variant_candidates(&self, symbol: &Symbol, nullary: bool) -> Vec<BindingId> {
        let mut candidates: Vec<BindingId> = self
            .table
            .visible_bindings(symbol.scopes(), Namespace::Value, self.phase)
            .into_iter()
            .filter(|binding| self.is_variant_candidate(*binding, symbol.name(), nullary))
            .collect();
        candidates.sort();
        candidates.dedup();
        candidates
    }

    fn is_variant_candidate(&self, binding: BindingId, name: &str, nullary: bool) -> bool {
        let BindingKind::Variant(_, info) = self.kind(binding) else {
            return false;
        };
        if nullary && info.arity != 0 {
            return false;
        }
        self.bindings
            .get(binding)
            .name()
            .rsplit('/')
            .next()
            .is_some_and(|short| short == name)
    }

    fn lower_rust(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let items = shape.exactly(3)?;
        let text = shape.string(&items[1])?;
        let template = ParsedTemplate::parse(text)
            .map_err(|_| ExpandProblems::malformed_template(items[1].span()))?;
        let type_expr = self.lower_type(&items[2])?;
        self.check_template_type_placeholders(&template, &type_expr, items[1].span())?;
        Ok(ExprKind::Leaf(Leaf::Rust(Rc::new(RustTemplate {
            template: text.into(),
            type_expr,
        }))))
    }

    fn check_template_type_placeholders(
        &self,
        template: &ParsedTemplate,
        type_expr: &TypeExpr,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let binders = match type_expr.kind() {
            TypeExprKind::Where(binders, _) => binders.as_slice(),
            _ => &[],
        };
        let parameters: Vec<String> = binders
            .iter()
            .map(|binder| self.bindings.get(binder.variable).name().to_string())
            .collect();
        for name in template.type_placeholders() {
            if !parameters.iter().any(|parameter| parameter == name) {
                return Err(ExpandProblems::unknown_type_placeholder(
                    name,
                    &parameters,
                    span,
                ));
            }
        }
        Ok(())
    }

    fn lower_recur(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let arguments = shape.items()[1..]
            .iter()
            .map(|argument| self.lower_recovered(argument))
            .collect::<Result<Vec<Expr>, Diagnostic>>()?;
        Ok(ExprKind::Recur(arguments))
    }

    fn lower_try(&mut self, shape: &Shape) -> Result<ExprKind, Diagnostic> {
        let items = shape.exactly(2)?;
        let value = self.lower_recovered(&items[1])?;
        Ok(ExprKind::Try(Box::new(value)))
    }
}
