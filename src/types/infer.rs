mod shared_variables;

use std::collections::HashMap;
use std::rc::Rc;

use super::checker::{TypeChecker, VariantArmReference};
use super::pending::{DecidedBorrows, PendingBorrow};
use super::problems::{MismatchContext, TypeProblems};
use super::ty::Type;
use super::ty::TypeVar;
use super::typed::{
    Borrow, PendingModeCheck, TypedArgument, TypedArm, TypedBinder, TypedBlock, TypedBodyItem,
    TypedControl, TypedExpr, TypedExprKind, TypedLambda, TypedLeaf, TypedPattern, TypedPatternKind,
    TypedStructExpr,
};
use super::unifier::{LiteralKind, UnifyError};
use super::wrapper::FunctionValueWrapper;
use crate::core::{
    Arm, Binder, BindingId, BindingKind, Block, BodyItem, Control, Expr, ExprKind, FieldLabel,
    Lambda, Leaf, Literal, Pattern, PatternKind, PrimitiveType, ReferenceKind, RustTemplate,
    StructExpr, VariantInfo,
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

struct BareNamePattern<'a> {
    binding: BindingId,
    name: &'a str,
    candidates: &'a [BindingId],
    outer: Option<BindingId>,
    matched: Type,
    mode: Option<ReferenceKind>,
    span: Span,
}

#[derive(Clone)]
pub(super) struct RecurTarget {
    parameters: Vec<Type>,
    result: Type,
}

#[derive(Clone)]
pub(super) struct TryTarget {
    returns: Type,
}

#[derive(Clone, Copy)]
enum TrySourceKind {
    Result,
    Option,
}

struct TrySource {
    kind: TrySourceKind,
    binding: BindingId,
    value: Type,
    error: Option<Type>,
}

impl TrySourceKind {
    fn needed(self, name: &str) -> String {
        match self {
            TrySourceKind::Result => format!("`{name}` with the same error type"),
            TrySourceKind::Option => format!("`{name}`"),
        }
    }
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
            ExprKind::Try(value) => self.infer_try(value, span),
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
            Control::Match(value, arms) => self.infer_match(value, arms, span),
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
        let payload = self.visible_payloads(owner, index, &type_arguments);
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

    fn infer_match(
        &mut self,
        value: &Expr,
        arms: &[Arm],
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        let value = self.infer(value)?;
        self.bind_match_enum(&value.ty, arms, value.span)?;
        let pattern_type = self.pattern_type(&value.ty, arms);
        let mut typed_arms = Vec::new();
        let mut result: Option<Type> = None;
        for arm in arms {
            let pattern = self.lower_pattern(&arm.pattern, &pattern_type, None)?;
            let (body, body_type) = self.infer_block(&arm.body)?;
            if let Some(first) = &result {
                let span = Self::block_result_span(&body).unwrap_or(arm.span);
                self.expect(first, &body_type, span, MismatchContext::Plain)?;
            } else {
                result = Some(body_type);
            }
            typed_arms.push(TypedArm {
                pattern,
                body,
                span: arm.span,
            });
        }
        self.check_match_coverage(span, &value.ty, &typed_arms);
        Ok(TypedExpr {
            kind: TypedExprKind::Control(TypedControl::Match(Box::new(value), typed_arms)),
            ty: result.unwrap_or_else(Type::unit),
            span,
        })
    }

    fn bind_match_enum(
        &mut self,
        scrutinee: &Type,
        arms: &[Arm],
        span: Span,
    ) -> Result<Option<BindingId>, Diagnostic> {
        let (matched, _) = self.peeled_reference(scrutinee);
        match self.unifier.shallow(&matched) {
            Type::Named(owner, _) if self.enums.get(owner).is_some() => Ok(Some(owner)),
            Type::Var(_) => {
                let Some(owner) = self.unknown_match_enum(arms)? else {
                    return Ok(None);
                };
                let expected = Type::Named(owner, self.fresh_arguments(owner));
                self.expect(&expected, &matched, span, MismatchContext::Plain)?;
                Ok(Some(owner))
            }
            _ => Ok(None),
        }
    }

    fn pattern_type(&self, scrutinee: &Type, arms: &[Arm]) -> Type {
        let strings = arms
            .iter()
            .any(|arm| matches!(arm.pattern.kind, PatternKind::Literal(Literal::String(_))));
        if strings && Self::is_string_value(scrutinee) {
            Type::str_ref()
        } else {
            scrutinee.clone()
        }
    }

    fn is_string_value(ty: &Type) -> bool {
        match ty {
            Type::Primitive(PrimitiveType::String) => true,
            Type::Ref(inner) => matches!(inner.as_ref(), Type::Primitive(PrimitiveType::String)),
            _ => false,
        }
    }

    fn unknown_match_enum(&self, arms: &[Arm]) -> Result<Option<BindingId>, Diagnostic> {
        let mut fitting: Option<Vec<BindingId>> = None;
        let mut first: Option<(Rc<str>, Span)> = None;
        for arm in arms {
            self.collect_arm_candidates(&arm.pattern, &mut fitting, &mut first);
        }
        let Some(mut fitting) = fitting else {
            return Ok(None);
        };
        fitting.sort();
        fitting.dedup();
        match fitting.as_slice() {
            [] => Ok(None),
            [owner] => Ok(Some(*owner)),
            _ => {
                let (name, span) = first.expect("a fitting enum comes from a variant pattern");
                let names: Vec<String> = fitting
                    .iter()
                    .map(|owner| self.bindings.get(*owner).name().to_string())
                    .collect();
                Err(TypeProblems::ambiguous_variant(&name, &names, span))
            }
        }
    }

    fn collect_arm_candidates(
        &self,
        pattern: &Pattern,
        fitting: &mut Option<Vec<BindingId>>,
        first: &mut Option<(Rc<str>, Span)>,
    ) {
        let (name, candidates): (&str, Vec<BindingId>) = match &pattern.kind {
            PatternKind::Variant(binding, _) => (self.variant_name(*binding), vec![*binding]),
            PatternKind::ShortVariant {
                name, candidates, ..
            }
            | PatternKind::BareName {
                name, candidates, ..
            } => (name, candidates.clone()),
            _ => return,
        };
        let owners: Vec<BindingId> = candidates
            .iter()
            .map(|binding| self.variant_owner(*binding))
            .collect();
        *fitting = Some(match fitting.take() {
            Some(current) => current
                .into_iter()
                .filter(|owner| owners.contains(owner))
                .collect(),
            None => owners,
        });
        first.get_or_insert_with(|| (name.into(), pattern.span));
    }

    fn variant_owner(&self, binding: BindingId) -> BindingId {
        match self.bindings.get(binding).kind() {
            BindingKind::Variant(_, info) => info.owner,
            _ => unreachable!("a variant pattern names a variant binding"),
        }
    }

    fn variant_name(&self, binding: BindingId) -> &str {
        self.bindings
            .get(binding)
            .name()
            .rsplit('/')
            .next()
            .unwrap_or_default()
    }

    fn lower_pattern(
        &mut self,
        pattern: &Pattern,
        expected: &Type,
        inherited: Option<ReferenceKind>,
    ) -> Result<TypedPattern, Diagnostic> {
        let (matched, own_mode) = self.peeled_reference(expected);
        let mode = own_mode.or(inherited);
        let kind = match &pattern.kind {
            PatternKind::Wildcard => Ok(TypedPatternKind::Wildcard),
            PatternKind::Bind(binding) => {
                Ok(self.bind_pattern(*binding, matched, mode, pattern.span))
            }
            PatternKind::BareName {
                name,
                binding,
                candidates,
                outer,
            } => self.bare_name_pattern(BareNamePattern {
                binding: *binding,
                name,
                candidates,
                outer: *outer,
                matched,
                mode,
                span: pattern.span,
            }),
            PatternKind::Variant(binding, patterns) => {
                self.named_variant_pattern(*binding, patterns, matched, mode, pattern.span)
            }
            PatternKind::ShortVariant {
                name,
                candidates,
                arguments,
            } => {
                self.short_variant_pattern(name, candidates, arguments, matched, mode, pattern.span)
            }
            PatternKind::Literal(literal) => {
                self.literal_pattern(literal, &matched, expected, pattern.span)
            }
        };
        kind.map(|kind| TypedPattern {
            kind,
            span: pattern.span,
        })
    }

    fn bare_name_pattern(
        &mut self,
        pattern: BareNamePattern<'_>,
    ) -> Result<TypedPatternKind, Diagnostic> {
        match self.nullary_variant(
            pattern.name,
            pattern.candidates,
            &pattern.matched,
            pattern.span,
        )? {
            Some(variant) => {
                self.variant_arm.insert(
                    pattern.binding,
                    VariantArmReference {
                        outer: pattern.outer,
                        variant,
                    },
                );
                Ok(TypedPatternKind::Variant(variant, Vec::new()))
            }
            None => {
                Ok(self.bind_pattern(pattern.binding, pattern.matched, pattern.mode, pattern.span))
            }
        }
    }

    fn nullary_variant(
        &mut self,
        name: &str,
        candidates: &[BindingId],
        matched: &Type,
        span: Span,
    ) -> Result<Option<BindingId>, Diagnostic> {
        if let Type::Named(owner, _) = self.unifier.shallow(matched)
            && self.enums.get(owner).is_some()
        {
            return Ok(self.find_nullary_variant(owner, name, candidates));
        }
        if !matches!(self.unifier.shallow(matched), Type::Var(_)) {
            return Ok(None);
        }
        let owner = self.unique_candidate_owner(candidates, span)?;
        let arguments = self.fresh_arguments(owner);
        let expected = Type::Named(owner, arguments);
        self.expect(&expected, matched, span, MismatchContext::Plain)?;
        Ok(self.find_nullary_variant(owner, name, candidates))
    }

    fn find_nullary_variant(
        &self,
        owner: BindingId,
        name: &str,
        candidates: &[BindingId],
    ) -> Option<BindingId> {
        let definition = self.enums.get(owner)?;
        let variant = definition
            .variants
            .iter()
            .find(|variant| variant.name.as_ref() == name)?;
        if !matches!(variant.payload, super::enums::VariantPayload::None) {
            return None;
        }
        candidates.iter().copied().find(|binding| {
            matches!(
                self.bindings.get(*binding).kind(),
                BindingKind::Variant(_, info) if info.owner == owner
            )
        })
    }

    fn literal_pattern(
        &mut self,
        literal: &Literal,
        matched: &Type,
        expected: &Type,
        span: Span,
    ) -> Result<TypedPatternKind, Diagnostic> {
        let literal_type = self.literal_type(literal);
        let matched = self.unifier.resolve(matched);
        if matches!(literal, Literal::String(_)) && Self::is_string_position(&matched) {
            return Ok(TypedPatternKind::Literal(literal.clone(), matched));
        }
        let compared = match literal {
            Literal::String(_) => expected,
            _ => &matched,
        };
        self.expect(compared, &literal_type, span, MismatchContext::Plain)?;
        Ok(TypedPatternKind::Literal(
            literal.clone(),
            self.unifier.resolve(&literal_type),
        ))
    }

    fn is_string_position(ty: &Type) -> bool {
        matches!(
            ty,
            Type::Primitive(PrimitiveType::String | PrimitiveType::Str)
        )
    }

    fn bind_pattern(
        &mut self,
        binding: BindingId,
        matched: Type,
        mode: Option<ReferenceKind>,
        span: Span,
    ) -> TypedPatternKind {
        let ty = Self::binding_mode_type(matched, mode);
        self.locals.insert(binding, ty.clone());
        TypedPatternKind::Bind(TypedBinder {
            binding,
            ty,
            span,
            annotated: false,
        })
    }

    fn named_variant_pattern(
        &mut self,
        binding: BindingId,
        patterns: &[Pattern],
        matched: Type,
        mode: Option<ReferenceKind>,
        span: Span,
    ) -> Result<TypedPatternKind, Diagnostic> {
        let owner = self.variant_owner(binding);
        let arguments = self.match_arguments(matched, owner, span)?;
        let index = match self.bindings.get(binding).kind() {
            BindingKind::Variant(_, info) => info.index,
            _ => unreachable!("a qualified pattern names a variant binding"),
        };
        let storage = self
            .enums
            .get(owner)
            .and_then(|definition| definition.instantiated_payload(index, &arguments))
            .unwrap_or_default();
        self.check_pattern_count(
            self.variant_name(binding),
            storage.len(),
            patterns.len(),
            span,
        )?;
        let typed = self.lower_payload_patterns(patterns, &storage, mode)?;
        Ok(TypedPatternKind::Variant(binding, typed))
    }

    fn short_variant_pattern(
        &mut self,
        name: &str,
        candidates: &[BindingId],
        arguments: &[Pattern],
        matched: Type,
        mode: Option<ReferenceKind>,
        span: Span,
    ) -> Result<TypedPatternKind, Diagnostic> {
        let (owner, type_arguments) = self.short_pattern_owner(candidates, &matched, span)?;
        let definition = self
            .enums
            .get(owner)
            .expect("a matching enum has a definition");
        let Some(index) = definition
            .variants
            .iter()
            .position(|variant| variant.name.as_ref() == name)
        else {
            let variants: Vec<Rc<str>> = definition
                .variants
                .iter()
                .map(|variant| variant.name.clone())
                .collect();
            let enum_name = self.bindings.get(owner).name();
            return Err(TypeProblems::unknown_variant(
                enum_name, name, &variants, span,
            ));
        };
        let storage = self
            .enums
            .get(owner)
            .and_then(|definition| definition.instantiated_payload(index, &type_arguments))
            .unwrap_or_default();
        let binding = self.candidate_binding(candidates, owner, index);
        self.check_pattern_count(name, storage.len(), arguments.len(), span)?;
        let typed = self.lower_payload_patterns(arguments, &storage, mode)?;
        Ok(TypedPatternKind::Variant(binding, typed))
    }

    fn lower_payload_patterns(
        &mut self,
        patterns: &[Pattern],
        storage: &[Type],
        mode: Option<ReferenceKind>,
    ) -> Result<Vec<TypedPattern>, Diagnostic> {
        patterns
            .iter()
            .zip(storage)
            .map(|(pattern, storage)| {
                let payload = self.visible_payload(storage);
                let typed = self.lower_pattern(pattern, &payload, mode)?;
                if self.is_box(storage)
                    && matches!(
                        typed.kind,
                        TypedPatternKind::Variant(..) | TypedPatternKind::Literal(..)
                    )
                {
                    return Err(TypeProblems::nested_boxed_pattern(pattern.span));
                }
                Ok(typed)
            })
            .collect()
    }

    fn short_pattern_owner(
        &mut self,
        candidates: &[BindingId],
        matched: &Type,
        span: Span,
    ) -> Result<(BindingId, Vec<Type>), Diagnostic> {
        if let Type::Named(owner, arguments) = self.unifier.shallow(matched)
            && self.enums.get(owner).is_some()
        {
            return Ok((owner, arguments));
        }
        let owner = if matches!(self.unifier.shallow(matched), Type::Var(_)) {
            self.unique_candidate_owner(candidates, span)?
        } else {
            self.candidate_owners(candidates)[0]
        };
        let arguments = self.fresh_arguments(owner);
        let expected = Type::Named(owner, arguments.clone());
        self.expect(&expected, matched, span, MismatchContext::Plain)?;
        Ok((owner, arguments))
    }

    fn candidate_owners(&self, candidates: &[BindingId]) -> Vec<BindingId> {
        let mut owners: Vec<BindingId> = candidates
            .iter()
            .map(|binding| self.variant_owner(*binding))
            .collect();
        owners.sort();
        owners.dedup();
        owners
    }

    fn unique_candidate_owner(
        &self,
        candidates: &[BindingId],
        span: Span,
    ) -> Result<BindingId, Diagnostic> {
        let owners = self.candidate_owners(candidates);
        match owners.as_slice() {
            [owner] => Ok(*owner),
            _ => {
                let names: Vec<String> = owners
                    .iter()
                    .map(|owner| self.bindings.get(*owner).name().to_string())
                    .collect();
                Err(TypeProblems::ambiguous_variant("", &names, span))
            }
        }
    }

    fn candidate_binding(
        &self,
        candidates: &[BindingId],
        owner: BindingId,
        index: usize,
    ) -> BindingId {
        *candidates
            .iter()
            .find(|binding| {
                matches!(
                    self.bindings.get(**binding).kind(),
                    BindingKind::Variant(_, info) if info.owner == owner && info.index == index
                )
            })
            .unwrap_or(&candidates[0])
    }

    fn check_pattern_count(
        &self,
        name: &str,
        expected: usize,
        found: usize,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if expected == found {
            return Ok(());
        }
        Err(TypeProblems::pattern_count(name, expected, found, span))
    }

    fn match_arguments(
        &mut self,
        matched: Type,
        owner: BindingId,
        span: Span,
    ) -> Result<Vec<Type>, Diagnostic> {
        match self.unifier.shallow(&matched) {
            Type::Named(binding, arguments) if binding == owner => Ok(arguments),
            Type::Var(_) => {
                let arguments = self.fresh_arguments(owner);
                let named = Type::Named(owner, arguments.clone());
                self.expect(&named, &matched, span, MismatchContext::Plain)?;
                Ok(arguments)
            }
            found => {
                let expected = Type::Named(owner, self.fresh_arguments(owner));
                self.expect(&expected, &found, span, MismatchContext::Plain)?;
                Ok(Vec::new())
            }
        }
    }

    pub(super) fn peeled_reference(&self, ty: &Type) -> (Type, Option<ReferenceKind>) {
        let mut matched = self.unifier.resolve(ty);
        let mut mode = None;
        loop {
            match matched.clone() {
                Type::Ref(inner) => {
                    mode.get_or_insert(ReferenceKind::Shared);
                    matched = self.unifier.resolve(&inner);
                }
                Type::Mut(inner) => {
                    mode.get_or_insert(ReferenceKind::Mutable);
                    matched = self.unifier.resolve(&inner);
                }
                _ => return (matched, mode),
            }
        }
    }

    fn binding_mode_type(matched: Type, mode: Option<ReferenceKind>) -> Type {
        match mode {
            Some(ReferenceKind::Shared) => Type::Ref(Box::new(matched)),
            Some(ReferenceKind::Mutable) => Type::Mut(Box::new(matched)),
            None => matched,
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
        if self.coerces(expected, found, span) {
            return Ok(());
        }
        self.unifier
            .unify(expected, found, span)
            .map_err(|error| self.unify_problem(error, expected, found, span, context))
    }

    fn coerces(&mut self, expected: &Type, found: &Type, span: Span) -> bool {
        match (self.unifier.resolve(expected), self.unifier.resolve(found)) {
            (Type::Named(slice, slice_arguments), Type::Named(vector, vector_arguments))
                if self.rust_types.is_slice(slice) && self.rust_types.is_vector(vector) =>
            {
                self.unifier
                    .unify(&slice_arguments[0], &vector_arguments[0], span)
                    .is_ok()
            }
            (Type::Ref(slice), Type::Ref(vector)) => self.coerces(&slice, &vector, span),
            _ => false,
        }
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
        if let Some(reference) = self.variant_arm.get(&binding).copied() {
            return match reference.outer {
                Some(outer) => self.infer_variable(outer, span),
                None => {
                    let name = self.bindings.get(binding).name().to_string();
                    let variant = self.bindings.get(reference.variant).name().to_string();
                    Err(TypeProblems::variant_arm_value(&name, &variant, span))
                }
            };
        }
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
        let previous_try = self.try_target.replace(TryTarget {
            returns: result.clone(),
        });
        let inferred = self.infer_block(&lambda.body);
        self.recur_target = previous;
        self.try_target = previous_try;
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

    fn infer_try(&mut self, value: &Expr, span: Span) -> Result<TypedExpr, Diagnostic> {
        let target = self
            .try_target
            .clone()
            .expect("expansion checks that `try` is inside a function");
        let value = self.infer(value)?;
        let operand = self.unifier.resolve(&value.ty);
        let Some(source) = self.try_source(&operand) else {
            return Err(TypeProblems::try_needs_value(
                &self.describe(&operand),
                span,
            ));
        };
        let expected = self.try_return_type(&source);
        if self
            .unifier
            .unify(&expected, &target.returns, span)
            .is_err()
        {
            let source_name = self.bindings.get(source.binding).name();
            return Err(TypeProblems::try_cannot_return(
                &source.kind.needed(source_name),
                &self.describe(&target.returns),
                span,
            ));
        }
        Ok(TypedExpr {
            kind: TypedExprKind::Try(Box::new(value)),
            ty: source.value,
            span,
        })
    }

    fn try_source(&self, operand: &Type) -> Option<TrySource> {
        let Type::Named(binding, arguments) = operand else {
            return None;
        };
        let rust_name = self.rust_types.get(*binding)?.leading_name();
        match (rust_name, arguments.as_slice()) {
            ("Result", [value, error]) => Some(TrySource {
                kind: TrySourceKind::Result,
                binding: *binding,
                value: value.clone(),
                error: Some(error.clone()),
            }),
            ("Option", [value]) => Some(TrySource {
                kind: TrySourceKind::Option,
                binding: *binding,
                value: value.clone(),
                error: None,
            }),
            _ => None,
        }
    }

    fn try_return_type(&mut self, source: &TrySource) -> Type {
        let value = self.unifier.fresh();
        match source.kind {
            TrySourceKind::Result => Type::Named(
                source.binding,
                vec![
                    value,
                    source
                        .error
                        .clone()
                        .expect("a result source carries an error type"),
                ],
            ),
            TrySourceKind::Option => Type::Named(source.binding, vec![value]),
        }
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
        let parameter = self.unifier.shallow(parameter);
        let argument = self.slice_argument(&parameter, &value.ty, value.span);
        if value.is_written_form() {
            return self.pass(&parameter, &argument, value.span, Borrow::None);
        }
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

    fn slice_argument(&mut self, parameter: &Type, argument: &Type, span: Span) -> Type {
        let found = self.unifier.resolve(argument);
        match (parameter, &found) {
            (Type::Ref(expected), Type::Ref(referent)) => {
                Type::Ref(Box::new(self.vector_before_slice(expected, referent, span)))
            }
            (Type::Ref(expected), referent) if !referent.is_reference() => {
                self.vector_before_slice(expected, referent, span)
            }
            _ => found,
        }
    }

    fn vector_before_slice(&mut self, expected: &Type, found: &Type, span: Span) -> Type {
        let Type::Named(slice, _) = self.unifier.shallow(expected) else {
            return found.clone();
        };
        let Type::Var(_) = found else {
            return found.clone();
        };
        if !self.rust_types.is_slice(slice) {
            return found.clone();
        }
        let Some(vector) = self
            .rust_types
            .binding_with_template(crate::types::RustType::VECTOR_TEMPLATE)
        else {
            return found.clone();
        };
        let element = self.unifier.fresh();
        match self
            .unifier
            .unify(found, &Type::Named(vector, vec![element]), span)
        {
            Ok(()) => self.unifier.resolve(found),
            Err(_) => found.clone(),
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
