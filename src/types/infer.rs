use std::collections::HashMap;
use std::rc::Rc;

use super::checker::TypeChecker;
use super::problems::{MismatchContext, TypeProblems};
use super::ty::Type;
use super::typed::{
    Borrow, TypedArgument, TypedBinder, TypedBlock, TypedBodyItem, TypedExpr, TypedExprKind,
    TypedLambda,
};
use super::unifier::{LiteralKind, UnifyError};
use crate::core::{
    Binder, BindingId, BindingKind, Block, BodyItem, Control, Expr, ExprKind, FieldLabel, Lambda,
    Leaf, Literal, RustTemplate, StructExpr,
};
use crate::diagnostic::Diagnostic;
use crate::diagnostic::SimilarName;
use crate::reader::NumberValue;
use crate::source::Span;

struct ExpectedFunction<'a> {
    parameters: &'a [Type],
    result: &'a Type,
    context: MismatchContext,
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
                TypedExprKind::Literal(literal.clone()),
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
                    .map(|(block, ty)| (TypedExprKind::Block(block), ty));
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
            BindingKind::Local => {
                Ok((TypedExprKind::Local(binding), self.locals[&binding].clone()))
            }
            BindingKind::Global(_) if self.compile_time_only.contains(&binding) => {
                Err(TypeProblems::compile_time_only(&name, span))
            }
            BindingKind::Global(_) => {
                Ok((TypedExprKind::Global(binding), self.global_type(binding)))
            }
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
            let expected_parameter = expected
                .as_ref()
                .and_then(|expected| expected.parameters.get(position));
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
        expected: Option<&Type>,
    ) -> Result<TypedBinder, Diagnostic> {
        let ty = match (&param.annotation, expected) {
            (Some(annotation), _) => self.lower_type(annotation, &HashMap::new())?,
            (None, Some(expected)) => expected.clone(),
            (None, None) => self.unifier.fresh(),
        };
        self.locals.insert(param.binding, ty.clone());
        let written_by_signature = expected
            .is_some_and(|expected| !matches!(self.unifier.shallow(expected), Type::Var(_)));
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
            BodyItem::Let(binder, value) => {
                let value = self.infer(value)?;
                let ty = match &binder.annotation {
                    Some(annotation) => {
                        let annotated = self.lower_type(annotation, &HashMap::new())?;
                        self.expect(
                            &annotated,
                            &value.ty,
                            value.span,
                            MismatchContext::Annotation {
                                annotation: annotation.span(),
                            },
                        )?;
                        annotated
                    }
                    None => value.ty.clone(),
                };
                self.locals.insert(binder.binding, ty.clone());
                let binder = TypedBinder {
                    binding: binder.binding,
                    ty,
                    span: binder.span,
                    annotated: binder.annotation.is_some(),
                };
                Ok((TypedBodyItem::Let(binder, value), Type::unit()))
            }
        }
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
            let value = self.infer(argument)?;
            let borrow = self.pass_argument(parameter, &value)?;
            typed_arguments.push(TypedArgument { value, borrow });
        }
        Ok(TypedExpr {
            kind: TypedExprKind::Call(Box::new(callee), typed_arguments),
            ty: result,
            span,
        })
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
        let argument = self.unifier.shallow(&value.ty);
        let parameter = self.unifier.shallow(parameter);
        match (&parameter, &argument) {
            (Type::Ref(inner), found) if !found.is_reference() => {
                self.pass(inner, found, value.span, Borrow::Shared)
            }
            (Type::Mut(inner), found) if !found.is_reference() => {
                self.pass(inner, found, value.span, Borrow::Mutable)
            }
            (expected, Type::Ref(inner)) if Self::is_known_value(expected) => {
                let borrow = if self.copy_rules().is_copy(inner) {
                    Borrow::Deref
                } else {
                    Borrow::Passthrough
                };
                self.pass(expected, inner, value.span, borrow)
            }
            (expected, found) => self.pass(expected, found, value.span, Borrow::None),
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
        matches!(parameter, Type::Struct(_) | Type::Primitive(_))
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
            kind: TypedExprKind::If(Box::new(test), Box::new(then), Box::new(otherwise)),
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
            TypedExprKind::Block(block) => match block.items.last() {
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
        let declared: Vec<(Rc<str>, Type)> = self
            .structs
            .get(binding)
            .map(|definition| definition.fields.clone())
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
            TypedExprKind::New(binding, typed_fields),
            Type::Struct(binding),
        ))
    }

    fn infer_field(
        &mut self,
        value: &Expr,
        label: &FieldLabel,
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        let value = self.infer(value)?;
        let owner = self.field_owner(&value, label)?;
        let declared = self
            .structs
            .get(owner)
            .and_then(|definition| definition.field(&label.name))
            .cloned();
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
            kind: TypedExprKind::Field(Box::new(value), label.clone()),
            ty,
            span,
        })
    }

    fn field_owner(
        &mut self,
        value: &TypedExpr,
        label: &FieldLabel,
    ) -> Result<BindingId, Diagnostic> {
        let mut owner = self.unifier.shallow(&value.ty);
        while let Type::Ref(inner) | Type::Mut(inner) = owner {
            owner = self.unifier.shallow(&inner);
        }
        match owner {
            Type::Struct(binding) => Ok(binding),
            Type::Var(_) => self.unique_owner(&owner, label),
            other => Err(TypeProblems::no_fields(
                &self.display().show(&other),
                value.span,
            )),
        }
    }

    fn unique_owner(&mut self, owner: &Type, label: &FieldLabel) -> Result<BindingId, Diagnostic> {
        match self.structs.with_field(&label.name).as_slice() {
            [binding] => {
                self.expect(
                    &Type::Struct(*binding),
                    owner,
                    label.span,
                    MismatchContext::Plain,
                )?;
                Ok(*binding)
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
        Ok((TypedExprKind::Rust(template.clone()), ty))
    }
}
