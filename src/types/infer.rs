mod shared_variables;

use std::collections::HashMap;
use std::rc::Rc;

use super::checker::TypeChecker;
use super::pending::{DecidedBorrows, PendingBorrow};
use super::problems::{MismatchContext, TypeProblems};
use super::ty::Type;
use super::ty::TypeVar;
use super::typed::{
    Borrow, PendingModeCheck, TypedArgument, TypedBinder, TypedBlock, TypedBodyItem, TypedControl,
    TypedExpr, TypedExprKind, TypedLambda, TypedLeaf, TypedStructExpr,
};
use super::unifier::{LiteralKind, UnifyError};
use super::wrapper::FunctionValueWrapper;
use crate::core::{
    Binder, BindingId, BindingKind, Block, BodyItem, Control, Expr, ExprKind, FieldLabel, Lambda,
    Leaf, Literal, PrimitiveType, ReferenceKind, RustTemplate, StructExpr, VariantInfo,
};
use crate::diagnostic::Diagnostic;
use crate::diagnostic::SimilarName;
use crate::reader::NumberValue;
use crate::source::Span;
use shared_variables::SharedVariables;

struct ExpectedFunction<'a> {
    parameters: &'a [Type],
    result: &'a Type,
    context: MismatchContext,
    written: bool,
}

#[derive(Clone)]
pub(super) struct RecurTarget {
    parameters: Vec<Type>,
    result: Type,
}

impl TypeChecker {
    pub(super) fn infer(&mut self, expr: &Expr) -> Result<TypedExpr, Diagnostic> {
        let span = expr.span();
        match expr.kind() {
            ExprKind::Leaf(leaf) => self.infer_leaf(leaf, span),
            ExprKind::Fn(lambda) => Self::typed(self.infer_lambda(lambda, span, None), span),
            ExprKind::Call(callee, arguments) => self.infer_call(callee, arguments, span),
            ExprKind::Control(control) => self.infer_control(control, span),
            other => self.infer_nested(expr, other, span),
        }
    }

    fn infer_nested(
        &mut self,
        expr: &Expr,
        kind: &ExprKind,
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        match kind {
            ExprKind::Recur(arguments) => self.infer_recur(arguments, span),
            ExprKind::Borrow(..) | ExprKind::Deref(..) => self.infer_written(expr, span),
            ExprKind::Struct(structure) => self.infer_struct(structure, span),
            ExprKind::Variant(binding, arguments) => self.infer_variant(*binding, arguments, span),
            _ => unreachable!("the caller matches a nested expression"),
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

    fn infer_written(&mut self, expr: &Expr, span: Span) -> Result<TypedExpr, Diagnostic> {
        match expr.kind() {
            ExprKind::Borrow(kind, value) => self.infer_borrow(*kind, value, span),
            ExprKind::Deref(value) => self.infer_deref(value, span),
            _ => unreachable!("the caller matches a written form"),
        }
    }

    fn infer_borrow(
        &mut self,
        kind: ReferenceKind,
        value: &Expr,
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        let value = self.infer(value)?;
        if kind == ReferenceKind::Mutable && !value.is_place() {
            return Err(TypeProblems::mut_borrow_of_copy(
                value.span,
                self.field_place(&value),
            ));
        }
        let borrowed = self.place_type(&value);
        let ty = match kind {
            ReferenceKind::Shared => Type::Ref(Box::new(borrowed)),
            ReferenceKind::Mutable => Type::Mut(Box::new(borrowed)),
        };
        Ok(TypedExpr {
            kind: TypedExprKind::Borrow(kind, Box::new(value)),
            ty,
            span,
        })
    }

    fn infer_deref(&mut self, value: &Expr, span: Span) -> Result<TypedExpr, Diagnostic> {
        let value = self.infer(value)?;
        let referent = self.unifier.fresh();
        self.require_copy(&referent);
        if let Type::Mut(inner) = self.unifier.resolve(&value.ty) {
            self.expect(&referent, &inner, value.span, MismatchContext::Plain)?;
        } else {
            self.expect(
                &Type::Ref(Box::new(referent.clone())),
                &value.ty,
                value.span,
                MismatchContext::Plain,
            )?;
        }
        Ok(TypedExpr {
            kind: TypedExprKind::Deref(Box::new(value)),
            ty: referent,
            span,
        })
    }

    fn place_type(&self, value: &TypedExpr) -> Type {
        let TypedExprKind::Struct(TypedStructExpr::Field(owner, label)) = &value.kind else {
            return value.ty.clone();
        };
        let mut owner = self.unifier.resolve(&owner.ty);
        while let Type::Ref(inner) | Type::Mut(inner) = owner {
            owner = self.unifier.resolve(&inner);
        }
        let Type::Named(binding, arguments) = owner else {
            return value.ty.clone();
        };
        self.structs
            .get(binding)
            .and_then(|definition| definition.instantiated_field(&label.name, &arguments))
            .unwrap_or_else(|| value.ty.clone())
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

    fn infer_variant(
        &mut self,
        binding: BindingId,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        let BindingKind::Variant(
            _,
            VariantInfo {
                owner,
                index,
                arity,
            },
        ) = self.bindings.get(binding).kind()
        else {
            unreachable!("the expander resolves a variant call to a variant binding");
        };
        let type_arguments = self.fresh_arguments(owner);
        let payload = self
            .enums
            .get(owner)
            .and_then(|definition| definition.instantiated_payload(index, &type_arguments))
            .unwrap_or_default();
        if arguments.len() != arity {
            return Err(TypeProblems::wrong_argument_count(
                arity,
                arguments.len(),
                span,
            ));
        }
        let mut typed = Vec::new();
        for (argument, expected) in arguments.iter().zip(&payload) {
            let value = self.infer(argument)?;
            self.expect(expected, &value.ty, value.span, MismatchContext::Plain)?;
            typed.push(value);
        }
        Ok(TypedExpr {
            kind: TypedExprKind::Variant(binding, typed),
            ty: Type::Named(owner, type_arguments),
            span,
        })
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
                    context: context.clone(),
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
        let result = match &expected {
            Some(expected) => expected.result.clone(),
            None => self.unifier.fresh(),
        };
        let target = RecurTarget {
            parameters: params.iter().map(|param| param.ty.clone()).collect(),
            result: result.clone(),
        };
        let previous = self.recur_target.replace(target);
        let inferred = self.infer_block(&lambda.body);
        self.recur_target = previous;
        let (body, found) = inferred?;
        let result_span = Self::block_result_span(&body).unwrap_or(span);
        let context = expected
            .as_ref()
            .map_or(MismatchContext::Plain, |expected| expected.context.clone());
        self.expect(&result, &found, result_span, context)?;
        let parameter_types = params.iter().map(|param| param.ty.clone()).collect();
        let function = Type::Function(parameter_types, Box::new(result));
        Ok((
            TypedExprKind::Fn(Rc::new(TypedLambda {
                params,
                body,
                wraps: None,
            })),
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
        if self.inferred_copies.contains(param.binding) {
            self.require_copy(&ty);
        }
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
        let shared = SharedVariables::of(&parameters, &self.unifier);
        let mut typed_arguments =
            self.infer_arguments(Some(&callee), &parameters, arguments, &shared)?;
        self.pass_shared_arguments(Some(&callee), &shared, &mut typed_arguments)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Call(Box::new(callee), typed_arguments),
            ty: result,
            span,
        })
    }

    fn infer_recur(&mut self, arguments: &[Expr], span: Span) -> Result<TypedExpr, Diagnostic> {
        let target = self
            .recur_target
            .clone()
            .expect("expansion checks that `recur` is inside a function");
        let shared = SharedVariables::of(&target.parameters, &self.unifier);
        let mut typed_arguments =
            self.infer_arguments(None, &target.parameters, arguments, &shared)?;
        self.pass_shared_arguments(None, &shared, &mut typed_arguments)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Recur(typed_arguments),
            ty: target.result,
            span,
        })
    }

    fn infer_arguments(
        &mut self,
        callee: Option<&TypedExpr>,
        parameters: &[Type],
        arguments: &[Expr],
        shared: &SharedVariables,
    ) -> Result<Vec<TypedArgument>, Diagnostic> {
        let mut typed_arguments = Vec::new();
        for (position, (parameter, argument)) in parameters.iter().zip(arguments).enumerate() {
            let value = self.infer_argument(parameter, argument)?;
            let borrow = if shared.contains(position) {
                Borrow::None
            } else {
                self.pass_call_argument(callee, position, parameter, &value)?
            };
            typed_arguments.push(TypedArgument { value, borrow });
        }
        Ok(typed_arguments)
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
        let TypedExprKind::Leaf(TypedLeaf::Global(binding)) = &value.kind else {
            return Ok(value);
        };
        let binding = *binding;
        let (Type::Function(expected, expected_result), Type::Function(found, found_result)) = (
            self.unifier.resolve(parameter),
            self.unifier.resolve(&value.ty),
        ) else {
            return Ok(value);
        };
        if expected.len() != found.len() || !Self::needs_wrapper(&expected, &found) {
            return Ok(value);
        }
        let mut wrapped =
            FunctionValueWrapper::new(&mut self.bindings).prepare(&expected, value, Some(binding));
        for argument in &mut wrapped.arguments {
            argument.borrow = Borrow::None;
        }
        for param in &wrapped.params {
            self.locals.insert(param.binding, param.ty.clone());
        }
        self.expect(
            expected_result.as_ref(),
            found_result.as_ref(),
            wrapped.span(),
            MismatchContext::Plain,
        )?;
        Ok(wrapped.finish(&expected_result, &found_result))
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

    fn pass_call_argument(
        &mut self,
        callee: Option<&TypedExpr>,
        position: usize,
        parameter: &Type,
        value: &TypedExpr,
    ) -> Result<Borrow, Diagnostic> {
        let call = match callee.map(|callee| &callee.kind) {
            Some(TypedExprKind::Leaf(TypedLeaf::Global(binding))) => *binding,
            _ => return self.pass_argument(parameter, value),
        };
        if let TypedExprKind::Borrow(kind, _) = &value.kind
            && !self.parameter_is_written(call, position)
            && !self.unifier.shallow(parameter).is_reference()
            && let Type::Ref(referent) | Type::Mut(referent) = &value.ty
        {
            self.pass(parameter, referent, value.span, Borrow::None)?;
            self.pending_mode_checks.push(PendingModeCheck {
                callee: call,
                position,
                kind: *kind,
                span: value.span,
            });
            return Ok(Borrow::None);
        }
        self.pass_argument(parameter, value)
    }

    fn parameter_is_written(&self, callee: BindingId, position: usize) -> bool {
        self.written_parameters
            .get(&callee)
            .and_then(|written| written.get(position))
            .copied()
            .unwrap_or(true)
    }

    fn pass_argument(&mut self, parameter: &Type, value: &TypedExpr) -> Result<Borrow, Diagnostic> {
        let argument = self.unifier.resolve(&value.ty);
        if value.is_written_form() {
            return self.pass(parameter, &argument, value.span, Borrow::None);
        }
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
            || self.unifier.bounds(var).iter().any(|bound| {
                self.copy_rules()
                    .bound_allows_only_copy_values(bound.binding)
            })
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
        if self
            .copy_rules()
            .is_small_copy(&self.unifier.resolve(referent))
        {
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
        if levels == 0
            && let Type::Ref(referent) = argument
            && self.passes_through(var, argument, referent)
        {
            return self.pass(&Type::Var(var), referent, span, Borrow::Passthrough);
        }
        let borrow = if levels == 0 {
            Borrow::None
        } else {
            Borrow::Deref(levels)
        };
        self.pass(&Type::Var(var), &passed.clone(), span, borrow)
    }

    fn dereferences(&self, var: TypeVar, reference: &Type, referent: &Type) -> bool {
        let fails_traits =
            self.unifier.literal_kind(var).is_some() || self.fails_bounds(var, reference);
        fails_traits && self.holds_copy_value(referent)
    }

    fn passes_through(&self, var: TypeVar, reference: &Type, referent: &Type) -> bool {
        self.fails_bounds(var, reference) && !self.fails_bounds(var, referent)
    }

    fn fails_bounds(&self, var: TypeVar, value: &Type) -> bool {
        self.unifier.bounds(var).iter().any(|bound| {
            !self.traits.satisfies(bound, value, &|generic, required| {
                self.unifier.generic_bounds(generic).contains(required)
            })
        })
    }

    fn holds_copy_value(&self, referent: &Type) -> bool {
        match referent {
            Type::Var(var) => self.unifier.literal_kind(*var).is_some(),
            _ => self.copy_rules().is_small_copy(referent),
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
        let expanded_from = self.macro_name(span);
        self.expect(
            &Type::bool(),
            &test.ty,
            test.span,
            MismatchContext::IfTest {
                expanded_from: expanded_from.clone(),
            },
        )?;
        let then = self.infer(then)?;
        let otherwise = self.infer(otherwise)?;
        let first = Self::result_span(&then);
        self.expect(
            &then.ty,
            &otherwise.ty,
            Self::result_span(&otherwise),
            MismatchContext::IfBranch {
                first,
                expanded_from,
            },
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
        let ty = if self.copy_rules().is_small_copy(&ty) {
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

#[cfg(test)]
mod tests {
    use super::TypeChecker;
    use crate::core::{
        BindingArena, BindingInfo, BindingKind, Expr, ExprKind, FieldLabel, Leaf, Literal,
        ModuleId, PrimitiveType,
    };
    use crate::source::{SourceId, Span};
    use crate::types::typed::{TypedExpr, TypedExprKind, TypedLeaf, TypedStructExpr};
    use crate::types::{Type, TypeVar};

    struct PlaceTypeFixture {
        checker: TypeChecker,
        span: Span,
    }

    impl PlaceTypeFixture {
        fn new() -> Self {
            Self {
                checker: TypeChecker::new(BindingArena::default()),
                span: Span::new(SourceId::new(0), 0..0),
            }
        }

        fn field_on(&self, owner: TypedExpr) -> TypedExpr {
            TypedExpr {
                kind: TypedExprKind::Struct(TypedStructExpr::Field(
                    Box::new(owner),
                    FieldLabel {
                        name: "field".into(),
                        span: self.span,
                    },
                )),
                ty: Type::Primitive(PrimitiveType::I64),
                span: self.span,
            }
        }

        fn global_without_a_scheme(&mut self) -> Type {
            let binding = self.checker.bindings.add(BindingInfo::new(
                "missing".into(),
                BindingKind::Global(ModuleId::new(0)),
                None,
            ));
            self.checker.global_type(binding)
        }

        fn rejects_a_compile_time_only_global(&mut self) -> bool {
            let binding = self.checker.bindings.add(BindingInfo::new(
                "hidden".into(),
                BindingKind::Global(ModuleId::new(0)),
                None,
            ));
            self.checker.compile_time_only.insert(binding);
            self.checker.infer_variable(binding, self.span).is_err()
        }

        fn describe_unknown(&mut self) -> String {
            let _ = self.checker.unifier.fresh();
            self.checker.describe(&Type::Var(TypeVar::new(0)))
        }

        fn owner(&self) -> TypedExpr {
            TypedExpr {
                kind: TypedExprKind::Leaf(TypedLeaf::Literal(Literal::Bool(false))),
                ty: Type::Primitive(PrimitiveType::I64),
                span: self.span,
            }
        }
    }

    #[test]
    fn a_global_without_a_scheme_gets_a_fresh_type() {
        let mut fixture = PlaceTypeFixture::new();
        assert!(matches!(fixture.global_without_a_scheme(), Type::Var(_)));
    }

    #[test]
    fn a_compile_time_only_global_is_rejected() {
        let mut fixture = PlaceTypeFixture::new();
        assert!(fixture.rejects_a_compile_time_only_global());
    }

    #[test]
    fn an_unknown_type_is_described_as_an_underscore() {
        let mut fixture = PlaceTypeFixture::new();
        assert_eq!(fixture.describe_unknown(), "`_`");
    }

    #[test]
    #[should_panic(expected = "the caller matches a written form")]
    fn inferring_a_non_written_expression_panics() {
        let mut fixture = PlaceTypeFixture::new();
        let expr = Expr::new(
            ExprKind::Leaf(Leaf::Literal(Literal::Bool(false))),
            fixture.span,
        );
        let _ = fixture.checker.infer_written(&expr, fixture.span);
    }

    #[test]
    fn a_field_of_a_non_struct_keeps_the_field_type() {
        let fixture = PlaceTypeFixture::new();
        let field = fixture.field_on(fixture.owner());
        assert_eq!(
            fixture.checker.place_type(&field),
            Type::Primitive(PrimitiveType::I64)
        );
    }
}
