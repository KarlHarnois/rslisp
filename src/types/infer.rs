use std::collections::HashMap;
use std::rc::Rc;

use super::checker::TypeChecker;
use super::pending::{DecidedBorrows, PendingBorrow};
use super::problems::{MismatchContext, TypeProblems};
use super::ty::Type;
use super::ty::TypeVar;
use super::typed::{
    Borrow, TypedArgument, TypedBinder, TypedBlock, TypedBodyItem, TypedControl, TypedExpr,
    TypedExprKind, TypedLambda, TypedLeaf, TypedStructExpr,
};
use super::unifier::{LiteralKind, UnifyError};
use crate::core::{
    Binder, BindingId, BindingInfo, BindingKind, Block, BodyItem, Control, Expr, ExprKind,
    FieldLabel, Lambda, Leaf, Literal, PrimitiveType, RustTemplate, StructExpr,
};
use crate::diagnostic::Diagnostic;
use crate::diagnostic::SimilarName;
use crate::reader::NumberValue;
use crate::source::Span;

struct ExpectedFunction<'a> {
    parameters: &'a [Type],
    result: &'a Type,
    context: MismatchContext,
    written: bool,
}

impl TypeChecker {
    pub(super) fn infer(&mut self, expr: &Expr) -> Result<TypedExpr, Diagnostic> {
        let span = expr.span();
        match expr.kind() {
            ExprKind::Leaf(leaf) => self.infer_leaf(leaf, span),
            ExprKind::Fn(lambda) => Self::typed(self.infer_lambda(lambda, span, None), span),
            ExprKind::Call(callee, arguments) => self.infer_call(callee, arguments, span),
            ExprKind::Control(control) => self.infer_control(control, span),
            ExprKind::Struct(structure) => self.infer_struct(structure, span),
        }
    }

    fn infer_leaf(&mut self, leaf: &Leaf, span: Span) -> Result<TypedExpr, Diagnostic> {
        let inferred = match leaf {
            Leaf::Literal(literal) => Ok((
                TypedExprKind::Leaf(TypedLeaf::Literal(literal.clone())),
                self.literal_type(literal),
            )),
            Leaf::Var(binding) => self.infer_variable(*binding, span),
            Leaf::Quote(_) => Err(TypeProblems::quote_at_runtime(span)),
            Leaf::Rust(template) => self.infer_inline_rust(template),
        };
        Self::typed(inferred, span)
    }

    fn infer_control(&mut self, control: &Control, span: Span) -> Result<TypedExpr, Diagnostic> {
        match control {
            Control::Block(block) => {
                let inferred = self
                    .infer_block(block)
                    .map(|(block, ty)| (TypedExprKind::Control(TypedControl::Block(block)), ty));
                Self::typed(inferred, span)
            }
            Control::If(test, then, otherwise) => self.infer_if(test, then, otherwise, span),
        }
    }

    fn infer_struct(
        &mut self,
        structure: &StructExpr,
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        match structure {
            StructExpr::New(binding, fields) => {
                Self::typed(self.infer_new(*binding, fields, span), span)
            }
            StructExpr::Field(value, label) => self.infer_field(value, label, span),
        }
    }

    fn typed(
        inferred: Result<(TypedExprKind, Type), Diagnostic>,
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        inferred.map(|(kind, ty)| TypedExpr { kind, ty, span })
    }

    pub(super) fn expect(
        &mut self,
        expected: &Type,
        found: &Type,
        span: Span,
        context: MismatchContext,
    ) -> Result<(), Diagnostic> {
        self.unifier
            .unify(expected, found, span)
            .map_err(|error| self.unify_problem(error, expected, found, span, context))
    }

    fn unify_problem(
        &self,
        error: UnifyError,
        expected: &Type,
        found: &Type,
        span: Span,
        context: MismatchContext,
    ) -> Diagnostic {
        let expected = self.unifier.resolve(expected);
        let found = self.unifier.resolve(found);
        match (error, &expected) {
            (UnifyError::Infinite, _) => TypeProblems::infinite(span),
            (_, Type::Generic(_)) => TypeProblems::generic_mismatch(
                &self.display().show(&expected),
                &self.describe(&found),
                span,
            ),
            _ => TypeProblems::mismatch(
                &self.describe(&expected),
                &self.describe(&found),
                span,
                context,
            ),
        }
    }

    pub(super) fn describe(&self, described: &Type) -> String {
        match described {
            Type::Var(var) => match self.unifier.literal_kind(*var) {
                Some(LiteralKind::Integer) => "an integer".to_string(),
                Some(LiteralKind::Float) => "a float".to_string(),
                None => "`_`".to_string(),
            },
            other => format!("`{}`", self.display().show(other)),
        }
    }

    fn literal_type(&mut self, literal: &Literal) -> Type {
        match literal {
            Literal::String(_) => Type::str_ref(),
            Literal::Bool(_) => Type::Primitive(PrimitiveType::Bool),
            Literal::Number(number) => match number.value() {
                NumberValue::Integer(_) => self.unifier.fresh_literal(LiteralKind::Integer),
                NumberValue::Float(_) => self.unifier.fresh_literal(LiteralKind::Float),
            },
        }
    }

    fn infer_variable(
        &mut self,
        binding: BindingId,
        span: Span,
    ) -> Result<(TypedExprKind, Type), Diagnostic> {
        let name = self.bindings.get(binding).name().to_string();
        match self.bindings.get(binding).kind() {
            BindingKind::Local => Ok((
                TypedExprKind::Leaf(TypedLeaf::Local(binding)),
                self.locals[&binding].clone(),
            )),
            BindingKind::Global(_) if self.compile_time_only.contains(&binding) => {
                Err(TypeProblems::compile_time_only(&name, span))
            }
            BindingKind::Global(_) => Ok((
                TypedExprKind::Leaf(TypedLeaf::Global(binding)),
                self.global_type(binding),
            )),
            _ => Err(TypeProblems::compile_time_only(&name, span)),
        }
    }

    fn global_type(&mut self, binding: BindingId) -> Type {
        if let Some(current) = self.component.get(&binding) {
            return current.clone();
        }
        match self.schemes.get(&binding).cloned() {
            Some(scheme) => self.instantiate(&scheme),
            None => self.unifier.fresh(),
        }
    }

    pub(super) fn infer_against(
        &mut self,
        expr: &Expr,
        expected: &Type,
        context: MismatchContext,
    ) -> Result<TypedExpr, Diagnostic> {
        let value = match (expr.kind(), self.unifier.shallow(expected)) {
            (ExprKind::Fn(lambda), Type::Function(parameters, result))
                if parameters.len() == lambda.params.len() =>
            {
                let expectation = ExpectedFunction {
                    parameters: &parameters,
                    result: &result,
                    context,
                    written: matches!(context, MismatchContext::Signature { .. }),
                };
                let (kind, ty) = self.infer_lambda(lambda, expr.span(), Some(expectation))?;
                TypedExpr {
                    kind,
                    ty,
                    span: expr.span(),
                }
            }
            _ => self.infer(expr)?,
        };
        self.expect(expected, &value.ty, value.span, context)?;
        Ok(value)
    }

    fn infer_lambda(
        &mut self,
        lambda: &Lambda,
        span: Span,
        expected: Option<ExpectedFunction>,
    ) -> Result<(TypedExprKind, Type), Diagnostic> {
        let mut params = Vec::new();
        for (position, param) in lambda.params.iter().enumerate() {
            let expected_parameter = expected.as_ref().and_then(|expected| {
                expected
                    .parameters
                    .get(position)
                    .map(|parameter| (parameter, expected.written))
            });
            params.push(self.bind_parameter(param, expected_parameter)?);
        }
        let (body, result) = self.infer_block(&lambda.body)?;
        if let Some(expected) = &expected {
            let result_span = Self::block_result_span(&body).unwrap_or(span);
            self.expect(expected.result, &result, result_span, expected.context)?;
        }
        let parameter_types = params.iter().map(|param| param.ty.clone()).collect();
        let function = Type::Function(parameter_types, Box::new(result));
        Ok((
            TypedExprKind::Fn(Rc::new(TypedLambda { params, body })),
            function,
        ))
    }

    fn bind_parameter(
        &mut self,
        param: &Binder,
        expected: Option<(&Type, bool)>,
    ) -> Result<TypedBinder, Diagnostic> {
        let ty = match (&param.annotation, expected) {
            (Some(annotation), _) => self.lower_type(annotation, &HashMap::new())?,
            (None, Some((expected, _))) => expected.clone(),
            (None, None) => self.unifier.fresh(),
        };
        self.locals.insert(param.binding, ty.clone());
        let written_by_signature = expected.is_some_and(|(expected, written)| {
            written && !matches!(self.unifier.shallow(expected), Type::Var(_))
        });
        Ok(TypedBinder {
            binding: param.binding,
            ty,
            span: param.span,
            annotated: param.annotation.is_some() || written_by_signature,
        })
    }

    fn infer_block(&mut self, block: &Block) -> Result<(TypedBlock, Type), Diagnostic> {
        let mut items = Vec::new();
        let mut result = Type::unit();
        for item in &block.items {
            let (typed, item_type) = self.infer_body_item(item)?;
            items.push(typed);
            result = item_type;
        }
        Ok((TypedBlock { items }, result))
    }

    fn infer_body_item(&mut self, item: &BodyItem) -> Result<(TypedBodyItem, Type), Diagnostic> {
        match item {
            BodyItem::Expr(expr) => {
                let typed = self.infer(expr)?;
                let ty = typed.ty.clone();
                Ok((TypedBodyItem::Expr(typed), ty))
            }
            BodyItem::Let(binder, value) => self.infer_let(binder, value),
        }
    }

    fn infer_let(
        &mut self,
        binder: &Binder,
        value: &Expr,
    ) -> Result<(TypedBodyItem, Type), Diagnostic> {
        let value = self.infer(value)?;
        let ty = self.binder_type(binder, &value)?;
        self.locals.insert(binder.binding, ty.clone());
        let binder = TypedBinder {
            binding: binder.binding,
            ty,
            span: binder.span,
            annotated: binder.annotation.is_some(),
        };
        Ok((TypedBodyItem::Let(binder, value), Type::unit()))
    }

    fn binder_type(&mut self, binder: &Binder, value: &TypedExpr) -> Result<Type, Diagnostic> {
        let Some(annotation) = &binder.annotation else {
            return Ok(value.ty.clone());
        };
        let annotated = self.lower_type(annotation, &HashMap::new())?;
        let context = MismatchContext::Annotation {
            annotation: annotation.span(),
        };
        self.expect(&annotated, &value.ty, value.span, context)?;
        Ok(annotated)
    }

    fn infer_call(
        &mut self,
        callee: &Expr,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        let callee = self.infer(callee)?;
        let (parameters, result) = self.function_shape(&callee, arguments.len())?;
        if parameters.len() != arguments.len() {
            return Err(TypeProblems::wrong_argument_count(
                parameters.len(),
                arguments.len(),
                span,
            ));
        }
        let mut typed_arguments = Vec::new();
        for (parameter, argument) in parameters.iter().zip(arguments) {
            let value = self.infer_argument(parameter, argument)?;
            let borrow = self.pass_argument(parameter, &value)?;
            typed_arguments.push(TypedArgument { value, borrow });
        }
        Ok(TypedExpr {
            kind: TypedExprKind::Call(Box::new(callee), typed_arguments),
            ty: result,
            span,
        })
    }

    fn infer_argument(
        &mut self,
        parameter: &Type,
        argument: &Expr,
    ) -> Result<TypedExpr, Diagnostic> {
        let span = argument.span();
        let ExprKind::Fn(lambda) = argument.kind() else {
            let value = self.infer(argument)?;
            return self.wrap_function_value(parameter, value);
        };
        match self.unifier.shallow(parameter) {
            Type::Function(parameters, result) if parameters.len() == lambda.params.len() => {
                let expectation = ExpectedFunction {
                    parameters: &parameters,
                    result: &result,
                    context: MismatchContext::Plain,
                    written: true,
                };
                let (kind, ty) = self.infer_lambda(lambda, span, Some(expectation))?;
                Ok(TypedExpr { kind, ty, span })
            }
            _ => self.infer(argument),
        }
    }

    fn wrap_function_value(
        &mut self,
        parameter: &Type,
        value: TypedExpr,
    ) -> Result<TypedExpr, Diagnostic> {
        let TypedExprKind::Leaf(TypedLeaf::Global(_)) = value.kind else {
            return Ok(value);
        };
        let (Type::Function(expected, expected_result), Type::Function(found, found_result)) = (
            self.unifier.resolve(parameter),
            self.unifier.resolve(&value.ty),
        ) else {
            return Ok(value);
        };
        if expected.len() != found.len() || !Self::needs_wrapper(&expected, &found) {
            return Ok(value);
        }
        let span = value.span;
        let mut params = Vec::new();
        let mut arguments = Vec::new();
        for (expected_param, found_param) in expected.iter().zip(&found) {
            let binding = self.bindings.add(BindingInfo::new(
                "argument".into(),
                BindingKind::Local,
                None,
            ));
            self.locals.insert(binding, expected_param.clone());
            let local = TypedExpr {
                kind: TypedExprKind::Leaf(TypedLeaf::Local(binding)),
                ty: expected_param.clone(),
                span,
            };
            let borrow = self.pass_argument(found_param, &local)?;
            params.push(TypedBinder {
                binding,
                ty: expected_param.clone(),
                span,
                annotated: false,
            });
            arguments.push(TypedArgument {
                value: local,
                borrow,
            });
        }
        self.expect(
            expected_result.as_ref(),
            found_result.as_ref(),
            span,
            MismatchContext::Plain,
        )?;
        let call = TypedExpr {
            kind: TypedExprKind::Call(Box::new(value), arguments),
            ty: found_result.as_ref().clone(),
            span,
        };
        let lambda = TypedLambda {
            params,
            body: TypedBlock {
                items: vec![TypedBodyItem::Expr(call)],
            },
        };
        Ok(TypedExpr {
            kind: TypedExprKind::Fn(Rc::new(lambda)),
            ty: Type::Function(expected, expected_result),
            span,
        })
    }

    fn needs_wrapper(expected: &[Type], found: &[Type]) -> bool {
        expected
            .iter()
            .zip(found)
            .any(|(expected, found)| expected.is_reference() && !found.is_reference())
    }

    fn function_shape(
        &mut self,
        callee: &TypedExpr,
        argument_count: usize,
    ) -> Result<(Vec<Type>, Type), Diagnostic> {
        match self.unifier.shallow(&callee.ty) {
            Type::Function(parameters, result) => Ok((parameters, *result)),
            Type::Var(var) if self.unifier.literal_kind(var).is_none() => {
                let parameters: Vec<Type> =
                    (0..argument_count).map(|_| self.unifier.fresh()).collect();
                let result = self.unifier.fresh();
                let shape = Type::Function(parameters.clone(), Box::new(result.clone()));
                self.expect(&shape, &callee.ty, callee.span, MismatchContext::Plain)?;
                Ok((parameters, result))
            }
            other => Err(TypeProblems::not_a_function(
                &self.describe(&other),
                callee.span,
            )),
        }
    }

    fn pass_argument(&mut self, parameter: &Type, value: &TypedExpr) -> Result<Borrow, Diagnostic> {
        let argument = self.unifier.resolve(&value.ty);
        let parameter = self.unifier.shallow(parameter);
        match (&parameter, &argument) {
            (Type::Ref(inner), found) if !found.is_reference() => {
                self.pass(inner, found, value.span, Borrow::Shared)
            }
            (Type::Mut(inner), found) if !found.is_reference() => {
                if !value.is_place() {
                    return Err(TypeProblems::mut_borrow_of_copy(
                        value.span,
                        self.field_place(value),
                    ));
                }
                self.pass(inner, found, value.span, Borrow::Mutable)
            }
            (Type::Var(var), Type::Ref(_)) if self.referent_unknown(&argument) => {
                Ok(self.defer_borrow(*var, &argument, value.span))
            }
            _ => self.pass_types(&parameter, &argument, value.span),
        }
    }

    fn defer_borrow(&mut self, var: TypeVar, argument: &Type, span: Span) -> Borrow {
        self.pending_borrows.push(PendingBorrow {
            var,
            argument: argument.clone(),
            span,
        });
        Borrow::Pending(self.pending_borrows.len() - 1)
    }

    fn referent_unknown(&self, argument: &Type) -> bool {
        matches!(
            Self::innermost_referent(argument),
            Type::Var(var) if self.unifier.literal_kind(*var).is_none()
        )
    }

    pub(super) fn decide_pending_borrows(&mut self) -> DecidedBorrows {
        let pending = std::mem::take(&mut self.pending_borrows);
        let mut decided = HashMap::new();
        for (id, pending) in pending.iter().enumerate() {
            let borrow = self.decide_borrow(pending).unwrap_or_else(|diagnostic| {
                self.diagnostics.push(diagnostic);
                Borrow::None
            });
            decided.insert(id, borrow);
        }
        DecidedBorrows::new(decided)
    }

    fn decide_borrow(&mut self, pending: &PendingBorrow) -> Result<Borrow, Diagnostic> {
        let argument = self.unifier.resolve(&pending.argument);
        let parameter = self.unifier.shallow(&Type::Var(pending.var));
        if let Type::Var(var) = parameter
            && self.referent_unknown(&argument)
            && self.needs_value(var)
        {
            let levels = Self::reference_depth(&argument);
            let referent = Self::innermost_referent(&argument).clone();
            return self.pass(&parameter, &referent, pending.span, Borrow::Deref(levels));
        }
        self.pass_types(&parameter, &argument, pending.span)
    }

    fn needs_value(&self, var: TypeVar) -> bool {
        self.unifier.literal_kind(var).is_some()
            || self
                .unifier
                .bounds(var)
                .iter()
                .any(|bound| self.copy_rules().bound_allows_only_copy_values(*bound))
    }

    fn pass_types(
        &mut self,
        parameter: &Type,
        argument: &Type,
        span: Span,
    ) -> Result<Borrow, Diagnostic> {
        match (parameter, argument) {
            (expected, Type::Ref(_)) if Self::is_known_value(expected) => {
                let levels = Self::reference_depth(argument);
                let referent = Self::innermost_referent(argument).clone();
                self.expect(expected, &referent, span, MismatchContext::Plain)?;
                Ok(self.borrow_of_referent(&referent, levels))
            }
            (Type::Ref(_), Type::Ref(_)) if self.collapses(parameter, argument) => {
                let parameter = self.unifier.resolve(parameter);
                let extra = Self::reference_depth(argument) - Self::reference_depth(&parameter);
                let stripped = Self::without_references(argument, extra);
                self.pass(&parameter, &stripped, span, Borrow::Collapse(extra))
            }
            (Type::Var(var), Type::Ref(_)) => self.pass_to_variable(*var, argument, span),
            (Type::Mut(_), Type::Mut(_)) => self.pass(parameter, argument, span, Borrow::Reborrow),
            (expected, found) => self.pass(expected, found, span, Borrow::None),
        }
    }

    fn collapses(&self, parameter: &Type, argument: &Type) -> bool {
        let parameter = self.unifier.resolve(parameter);
        Self::reference_depth(argument) > Self::reference_depth(&parameter)
            && !matches!(Self::innermost_referent(&parameter), Type::Var(_))
    }

    fn reference_depth(value: &Type) -> usize {
        match value {
            Type::Ref(inner) => 1 + Self::reference_depth(inner),
            _ => 0,
        }
    }

    fn innermost_referent(value: &Type) -> &Type {
        match value {
            Type::Ref(inner) => Self::innermost_referent(inner),
            _ => value,
        }
    }

    fn without_references(value: &Type, levels: usize) -> Type {
        match value {
            Type::Ref(inner) if levels > 0 => Self::without_references(inner, levels - 1),
            _ => value.clone(),
        }
    }

    fn field_place(&self, value: &TypedExpr) -> Option<String> {
        let TypedExprKind::Call(callee, arguments) = &value.kind else {
            return None;
        };
        let TypedExprKind::Leaf(TypedLeaf::Global(binding)) = callee.kind else {
            return None;
        };
        let label = self.field_accessors.get(&binding)?;
        let [argument] = arguments.as_slice() else {
            return None;
        };
        let place = self.local_place(&argument.value)?;
        Some(format!("(field {place} {label})"))
    }

    fn local_place(&self, value: &TypedExpr) -> Option<String> {
        match &value.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => {
                Some(self.bindings.get(*binding).name().to_string())
            }
            TypedExprKind::Struct(TypedStructExpr::Field(owner, label)) => Some(format!(
                "(field {} {})",
                self.local_place(owner)?,
                label.name
            )),
            _ => None,
        }
    }

    fn borrow_of_referent(&self, referent: &Type, levels: usize) -> Borrow {
        if self.copy_rules().is_copy(&self.unifier.resolve(referent)) {
            Borrow::Deref(levels)
        } else {
            Borrow::Passthrough
        }
    }

    fn pass_to_variable(
        &mut self,
        var: TypeVar,
        argument: &Type,
        span: Span,
    ) -> Result<Borrow, Diagnostic> {
        let mut levels = 0;
        let mut passed = argument;
        while let Type::Ref(referent) = passed
            && self.dereferences(var, passed, referent)
        {
            levels += 1;
            passed = referent;
        }
        let borrow = if levels == 0 {
            Borrow::None
        } else {
            Borrow::Deref(levels)
        };
        self.pass(&Type::Var(var), &passed.clone(), span, borrow)
    }

    fn dereferences(&self, var: TypeVar, reference: &Type, referent: &Type) -> bool {
        let fails_traits = self.unifier.literal_kind(var).is_some()
            || self.unifier.bounds(var).iter().any(|bound| {
                !self
                    .traits
                    .satisfies(*bound, reference, &|generic, required| {
                        self.unifier.generic_bounds(generic).contains(&required)
                    })
            });
        fails_traits && self.holds_copy_value(referent)
    }

    fn holds_copy_value(&self, referent: &Type) -> bool {
        match referent {
            Type::Var(var) => self.unifier.literal_kind(*var).is_some(),
            _ => self.copy_rules().is_copy(referent),
        }
    }

    fn pass(
        &mut self,
        expected: &Type,
        found: &Type,
        span: Span,
        borrow: Borrow,
    ) -> Result<Borrow, Diagnostic> {
        self.expect(expected, found, span, MismatchContext::Plain)
            .map(|()| borrow)
    }

    fn is_known_value(parameter: &Type) -> bool {
        matches!(parameter, Type::Named(..) | Type::Primitive(_))
    }

    fn infer_if(
        &mut self,
        test: &Expr,
        then: &Expr,
        otherwise: &Expr,
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        let test = self.infer(test)?;
        self.expect(&Type::bool(), &test.ty, test.span, MismatchContext::IfTest)?;
        let then = self.infer(then)?;
        let otherwise = self.infer(otherwise)?;
        let first = Self::result_span(&then);
        self.expect(
            &then.ty,
            &otherwise.ty,
            Self::result_span(&otherwise),
            MismatchContext::IfBranch { first },
        )?;
        let ty = then.ty.clone();
        Ok(TypedExpr {
            kind: TypedExprKind::Control(TypedControl::If(
                Box::new(test),
                Box::new(then),
                Box::new(otherwise),
            )),
            ty,
            span,
        })
    }

    fn block_result_span(block: &TypedBlock) -> Option<Span> {
        match block.items.last() {
            Some(TypedBodyItem::Expr(last)) => Some(Self::result_span(last)),
            _ => None,
        }
    }

    fn result_span(expr: &TypedExpr) -> Span {
        match &expr.kind {
            TypedExprKind::Control(TypedControl::Block(block)) => match block.items.last() {
                Some(TypedBodyItem::Expr(last)) => Self::result_span(last),
                _ => expr.span,
            },
            _ => expr.span,
        }
    }

    fn infer_new(
        &mut self,
        binding: BindingId,
        fields: &[(FieldLabel, Expr)],
        span: Span,
    ) -> Result<(TypedExprKind, Type), Diagnostic> {
        let struct_name = self.bindings.get(binding).name().to_string();
        let arguments = self.fresh_arguments(binding);
        let declared: Vec<(Rc<str>, Type)> = self
            .structs
            .get(binding)
            .map(|definition| definition.instantiated_fields(&arguments))
            .unwrap_or_default();
        let mut typed_fields = Vec::new();
        for (label, value) in fields {
            let Some((_, field_type)) = declared.iter().find(|(name, _)| *name == label.name)
            else {
                let names: Vec<Rc<str>> = declared.iter().map(|(name, _)| name.clone()).collect();
                return Err(TypeProblems::unknown_field(
                    &struct_name,
                    &label.name,
                    label.span,
                    SimilarName::closest(&label.name, &names),
                ));
            };
            let typed = self.infer(value)?;
            self.expect(field_type, &typed.ty, typed.span, MismatchContext::Plain)?;
            typed_fields.push((label.clone(), typed));
        }
        let missing: Vec<Rc<str>> = declared
            .iter()
            .map(|(name, _)| name.clone())
            .filter(|name| !fields.iter().any(|(label, _)| label.name == *name))
            .collect();
        if !missing.is_empty() {
            return Err(TypeProblems::missing_fields(&struct_name, &missing, span));
        }
        Ok((
            TypedExprKind::Struct(TypedStructExpr::New(binding, typed_fields)),
            Type::Named(binding, arguments),
        ))
    }

    fn fresh_arguments(&mut self, binding: BindingId) -> Vec<Type> {
        let count = self.type_arities.get(&binding).copied().unwrap_or_default();
        (0..count).map(|_| self.unifier.fresh()).collect()
    }

    fn infer_field(
        &mut self,
        value: &Expr,
        label: &FieldLabel,
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        let value = self.infer(value)?;
        let (owner, arguments) = self.field_owner(&value, label)?;
        let declared = self
            .structs
            .get(owner)
            .and_then(|definition| definition.instantiated_field(&label.name, &arguments));
        let Some(ty) = declared else {
            let struct_name = self.bindings.get(owner).name().to_string();
            let names: Vec<Rc<str>> = self
                .structs
                .get(owner)
                .map(|definition| {
                    definition
                        .fields
                        .iter()
                        .map(|(name, _)| name.clone())
                        .collect()
                })
                .unwrap_or_default();
            return Err(TypeProblems::unknown_field(
                &struct_name,
                &label.name,
                label.span,
                SimilarName::closest(&label.name, &names),
            ));
        };
        let ty = if self.copy_rules().is_copy(&ty) {
            ty
        } else {
            Type::Ref(Box::new(ty))
        };
        Ok(TypedExpr {
            kind: TypedExprKind::Struct(TypedStructExpr::Field(Box::new(value), label.clone())),
            ty,
            span,
        })
    }

    fn field_owner(
        &mut self,
        value: &TypedExpr,
        label: &FieldLabel,
    ) -> Result<(BindingId, Vec<Type>), Diagnostic> {
        let mut owner = self.unifier.shallow(&value.ty);
        while let Type::Ref(inner) | Type::Mut(inner) = owner {
            owner = self.unifier.shallow(&inner);
        }
        match owner {
            Type::Named(binding, arguments) if self.structs.get(binding).is_some() => {
                Ok((binding, arguments))
            }
            Type::Var(_) => self.unique_owner(&owner, label),
            other => Err(TypeProblems::no_fields(
                &self.display().show(&other),
                value.span,
            )),
        }
    }

    fn unique_owner(
        &mut self,
        owner: &Type,
        label: &FieldLabel,
    ) -> Result<(BindingId, Vec<Type>), Diagnostic> {
        match self.structs.with_field(&label.name).as_slice() {
            [binding] => {
                let arguments = self.fresh_arguments(*binding);
                let expected = Type::Named(*binding, arguments.clone());
                self.expect(&expected, owner, label.span, MismatchContext::Plain)?;
                Ok((*binding, arguments))
            }
            _ => Err(TypeProblems::ambiguous_field(&label.name, label.span)),
        }
    }

    fn infer_inline_rust(
        &mut self,
        template: &Rc<RustTemplate>,
    ) -> Result<(TypedExprKind, Type), Diagnostic> {
        let scheme = self.lower_scheme(&template.type_expr)?;
        let ty = self.instantiate(&scheme);
        Ok((TypedExprKind::Leaf(TypedLeaf::Rust(template.clone())), ty))
    }
}
