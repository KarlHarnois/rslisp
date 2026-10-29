mod shared_variables;

use std::collections::HashMap;
use std::rc::Rc;

use super::borrowed_items::ArgumentSite;
use super::checker::{TypeChecker, VariantArmReference};
use super::collection::{Collection, CollectionKind};
use super::enums::VariantPayload;
use super::pending::{DecidedBorrows, PendingBorrow};
use super::pipelines::Pipelines;
use super::problems::{MismatchContext, TypeProblems};
use super::rust_types::RustType;
use super::ty::Bound;
use super::ty::Type;
use super::ty::TypeVar;
use super::typed::{
    Borrow, PendingModeCheck, TryErrorConversion, TypedArgument, TypedArm, TypedBinder, TypedBlock,
    TypedBodyItem, TypedControl, TypedConversion, TypedExpr, TypedExprKind, TypedLambda, TypedLeaf,
    TypedMatch, TypedPattern, TypedPatternKind, TypedStructExpr,
};
use super::unifier::{LiteralKind, UnifyError};
use super::wrapper::FunctionValueWrapper;
use crate::core::{
    Arm, Binder, BindingId, BindingKind, Block, BodyItem, Control, Expr, ExprKind, FieldLabel,
    Keyword, Lambda, Leaf, Literal, MapOperation, Pattern, PatternKind, PrimitiveType,
    ReferenceKind, RustTemplate, StructExpr, TypeExpr, TypeExprKind, VariantInfo,
};
use crate::diagnostic::SimilarName;
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::reader::NumberValue;
use crate::source::Span;
use shared_variables::SharedVariables;

enum MapIterator {
    Keys,
    Vals,
    Entries,
}

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

#[derive(Clone, Copy, PartialEq)]
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

struct TryConversion {
    enum_binding: BindingId,
    variant: usize,
    payload: Type,
}

impl TrySourceKind {
    fn needed(self, name: &str) -> String {
        match self {
            TrySourceKind::Result => {
                format!("`{name}` whose error can hold this value's error")
            }
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
            _ => self.infer_composite(kind, span),
        }
    }

    fn infer_composite(&mut self, kind: &ExprKind, span: Span) -> Result<TypedExpr, Diagnostic> {
        match kind {
            ExprKind::Struct(structure) => self.infer_struct(structure, span),
            ExprKind::Variant(binding, arguments) => self.infer_variant(*binding, arguments, span),
            ExprKind::Assoc(value, keys, item) => self.infer_assoc(value, keys, item, span),
            ExprKind::MapAt(operation, value, arguments) => {
                self.infer_map_at(*operation, value, arguments, span)
            }
            ExprKind::SetAt(value, keys, item) => self.infer_set_at(value, keys, item, span),
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
            ReferenceKind::Shared => Type::shared(borrowed),
            ReferenceKind::Mutable => Type::mutable(borrowed),
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
        if let Type::Mut(_, inner) = self.unifier.resolve(&value.ty) {
            self.expect(&referent, &inner, value.span, MismatchContext::Plain)?;
        } else {
            self.expect(
                &Type::shared(referent.clone()),
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
        while let Type::Ref(_, inner) | Type::Mut(_, inner) = owner {
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
            StructExpr::Update(base, fields) => {
                Self::typed(self.infer_update(base, fields, span), span)
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
        self.infer_variant_expecting(binding, arguments, None, span)
    }

    fn infer_variant_expecting(
        &mut self,
        binding: BindingId,
        arguments: &[Expr],
        expected: Option<&Type>,
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
        let type_arguments = match expected.map(|expected| self.unifier.shallow(expected)) {
            Some(Type::Named(expected_owner, arguments)) if expected_owner == owner => arguments,
            _ => self.fresh_arguments(owner),
        };
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
            let value = self.infer_against(argument, expected, MismatchContext::Plain)?;
            typed.push(value);
        }
        if let Some(value) = self.result_ok_try(owner, index, &typed) {
            return Ok(value);
        }
        Ok(TypedExpr {
            kind: TypedExprKind::Variant(binding, typed),
            ty: Type::Named(owner, type_arguments),
            span,
        })
    }

    fn result_ok_try(
        &self,
        owner: BindingId,
        index: usize,
        typed: &[TypedExpr],
    ) -> Option<TypedExpr> {
        if !self.is_result_ok_variant(owner, index) {
            return None;
        }
        let [
            TypedExpr {
                kind:
                    TypedExprKind::Try {
                        value,
                        conversion: TryErrorConversion::Identity,
                    },
                ..
            },
        ] = typed
        else {
            return None;
        };
        Some((**value).clone())
    }

    fn is_result_ok_variant(&self, owner: BindingId, index: usize) -> bool {
        let definition = self.enums.get(owner).expect("a variant owner is an enum");
        definition.is_result_shaped() && definition.holds_parameter(index, 0)
    }

    fn infer_match(
        &mut self,
        value: &Expr,
        arms: &[Arm],
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        self.infer_match_expecting(value, arms, None, span)
    }

    fn infer_match_expecting(
        &mut self,
        value: &Expr,
        arms: &[Arm],
        expected: Option<&Type>,
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        let value = self.infer(value)?;
        self.bind_match_enum(&value.ty, arms, value.span)?;
        let pattern_type = self.pattern_type(&value.ty, arms);
        let mut typed_arms = Vec::new();
        let mut result: Option<Type> = None;
        for arm in arms {
            let (mut typed_arm, body_type) = self.infer_arm(arm, &pattern_type, expected)?;
            let span = Self::block_result_span(&typed_arm.body).unwrap_or(typed_arm.span);
            self.record_arm_result(
                &mut result,
                &mut typed_arms,
                &mut typed_arm,
                &body_type,
                span,
            )?;
            typed_arms.push(typed_arm);
        }
        self.check_match_coverage(span, &value.ty, &typed_arms);
        let unguarded_exhaustive = self.match_is_exhaustive_without_guards(&value.ty, &typed_arms);
        Ok(TypedExpr {
            kind: TypedExprKind::Control(TypedControl::Match(TypedMatch {
                value: Box::new(value),
                arms: typed_arms,
                unguarded_exhaustive,
            })),
            ty: result.unwrap_or_else(Type::unit),
            span,
        })
    }

    fn infer_arm(
        &mut self,
        arm: &Arm,
        pattern_type: &Type,
        expected: Option<&Type>,
    ) -> Result<(TypedArm, Type), Diagnostic> {
        let pattern = self.lower_pattern(&arm.pattern, pattern_type, None)?;
        let (body, body_type) = match expected {
            Some(expected) => {
                self.infer_block_expecting(&arm.body, expected, MismatchContext::Plain)?
            }
            None => self.infer_block(&arm.body)?,
        };
        Ok((
            TypedArm {
                pattern,
                body,
                span: arm.span,
            },
            body_type,
        ))
    }

    fn record_arm_result(
        &mut self,
        result: &mut Option<Type>,
        earlier: &mut [TypedArm],
        arm: &mut TypedArm,
        body_type: &Type,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let Some(first) = result.clone() else {
            *result = Some(body_type.clone());
            return Ok(());
        };
        if let Some(target) = self.copy_target(&first, body_type, span) {
            for earlier_arm in earlier.iter_mut() {
                self.copy_arm_out(earlier_arm, &target);
            }
            *result = Some(target);
            return Ok(());
        }
        if let Some(target) = self.copy_target(body_type, &first, span) {
            self.copy_arm_out(arm, &target);
            *result = Some(target);
            return Ok(());
        }
        self.expect(&first, body_type, span, MismatchContext::Plain)
    }

    fn copy_target(&mut self, reference: &Type, value: &Type, span: Span) -> Option<Type> {
        let Type::Ref(_, inner) = self.unifier.resolve(reference) else {
            return None;
        };
        let value = self.unifier.resolve(value);
        let literal = self.is_literal(&value);
        if !literal && !self.copy_rules().is_small_copy(&value) {
            return None;
        }
        self.unifier.unify(inner.as_ref(), &value, span).ok()?;
        if literal && matches!(inner.as_ref(), Type::Var(_)) {
            return Some(value);
        }
        let target = self.unifier.resolve(&value);
        self.copy_rules().is_small_copy(&target).then_some(target)
    }

    fn is_literal(&self, value: &Type) -> bool {
        matches!(value, Type::Var(var) if self.unifier.literal_kind(*var).is_some())
    }

    fn copy_arm_out(&self, arm: &mut TypedArm, expected: &Type) {
        let Some(TypedBodyItem::Expr(value)) = arm.body.items.last_mut() else {
            return;
        };
        let span = value.span;
        *value = TypedExpr {
            kind: TypedExprKind::Deref(Box::new(value.clone())),
            ty: expected.clone(),
            span,
        };
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
            Type::Ref(_, inner) => matches!(inner.as_ref(), Type::Primitive(PrimitiveType::String)),
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
            PatternKind::Struct(binding, patterns) => {
                self.struct_pattern(*binding, patterns, matched, mode, pattern.span)
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
            boxed: false,
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
        let typed = self.lower_payload_patterns(owner, index, patterns, &storage, mode)?;
        Ok(TypedPatternKind::Variant(binding, typed))
    }

    fn struct_pattern(
        &mut self,
        binding: BindingId,
        patterns: &[Pattern],
        matched: Type,
        mode: Option<ReferenceKind>,
        span: Span,
    ) -> Result<TypedPatternKind, Diagnostic> {
        let arguments = self.match_arguments(matched, binding, span)?;
        let fields = self
            .structs
            .get(binding)
            .map(|definition| definition.instantiated_fields(&arguments))
            .unwrap_or_default();
        self.check_pattern_count(
            self.bindings.get(binding).name(),
            fields.len(),
            patterns.len(),
            span,
        )?;
        let typed = patterns
            .iter()
            .zip(&fields)
            .map(|(pattern, (_, field_type))| self.lower_pattern(pattern, field_type, mode))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        Ok(TypedPatternKind::Struct(binding, typed))
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
        let typed = self.lower_payload_patterns(owner, index, arguments, &storage, mode)?;
        Ok(TypedPatternKind::Variant(binding, typed))
    }

    fn lower_payload_patterns(
        &mut self,
        owner: BindingId,
        variant: usize,
        patterns: &[Pattern],
        storage: &[Type],
        mode: Option<ReferenceKind>,
    ) -> Result<Vec<TypedPattern>, Diagnostic> {
        let mut typed = Vec::with_capacity(patterns.len());
        for (payload, (pattern, storage)) in patterns.iter().zip(storage).enumerate() {
            let inferred = self.is_inferred_box(owner, variant, payload);
            let payload = match inferred {
                true => self
                    .unboxed(storage)
                    .expect("an inferred box has one type argument"),
                false => storage.clone(),
            };
            let mut value = self.lower_pattern(pattern, &payload, mode)?;
            value.boxed = inferred;
            typed.push(value);
        }
        Ok(typed)
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
                Type::Ref(_, inner) => {
                    mode.get_or_insert(ReferenceKind::Shared);
                    matched = self.unifier.resolve(&inner);
                }
                Type::Mut(_, inner) => {
                    mode.get_or_insert(ReferenceKind::Mutable);
                    matched = self.unifier.resolve(&inner);
                }
                _ => return (matched, mode),
            }
        }
    }

    fn binding_mode_type(matched: Type, mode: Option<ReferenceKind>) -> Type {
        match mode {
            Some(ReferenceKind::Shared) => Type::shared(matched),
            Some(ReferenceKind::Mutable) => Type::mutable(matched),
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
            (Type::Ref(_, shared), Type::Ref(_, owned)) => {
                Self::views_string(&shared, &owned) || self.coerces(&shared, &owned, span)
            }
            (Type::Ref(_, shared), Type::Mut(_, mutable)) => {
                self.unifier.unify(&shared, &mutable, span).is_ok()
            }
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
        let value = self.infer_expecting(expr, expected, &context)?;
        let value = self.copy_out_of_borrow(value, expected);
        self.expect(expected, &value.ty, value.span, context)?;
        Ok(value)
    }

    fn infer_expecting(
        &mut self,
        expr: &Expr,
        expected: &Type,
        context: &MismatchContext,
    ) -> Result<TypedExpr, Diagnostic> {
        match (expr.kind(), self.unifier.shallow(expected)) {
            (ExprKind::Fn(lambda), Type::Function(_, parameters, result))
                if parameters.len() == lambda.params.len() =>
            {
                let expectation = ExpectedFunction {
                    parameters: &parameters,
                    result: &result,
                    context: context.clone(),
                    written: matches!(context, MismatchContext::Signature { .. }),
                };
                let (kind, ty) = self.infer_lambda(lambda, expr.span(), Some(expectation))?;
                Ok(TypedExpr {
                    kind,
                    ty,
                    span: expr.span(),
                })
            }
            (ExprKind::Call(callee, arguments), _) => {
                self.infer_call_expecting(callee, arguments, Some(expected), expr.span())
            }
            (ExprKind::Control(Control::Match(value, arms)), _) => {
                self.infer_match_expecting(value, arms, Some(expected), expr.span())
            }
            (ExprKind::Variant(binding, arguments), _) => {
                self.infer_variant_expecting(*binding, arguments, Some(expected), expr.span())
            }
            _ => self.infer(expr),
        }
    }

    fn copy_out_of_borrow(&self, value: TypedExpr, expected: &Type) -> TypedExpr {
        if value.is_written_form() {
            return value;
        }
        let span = value.span;
        let expected = self.unifier.resolve(expected);
        let found = self.unifier.resolve(&value.ty);
        match found {
            Type::Ref(_, referent)
                if *referent == expected && self.copy_rules().is_small_copy(&expected) =>
            {
                TypedExpr {
                    kind: TypedExprKind::Deref(Box::new(value)),
                    ty: expected,
                    span,
                }
            }
            _ => value,
        }
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
        let enclosing_body = std::mem::replace(&mut self.checking_function_body, true);
        let previous_try = self.try_target.replace(TryTarget {
            returns: result.clone(),
        });
        let context = expected
            .as_ref()
            .map_or(MismatchContext::Plain, |expected| expected.context.clone());
        let inferred = self.infer_block_expecting(&lambda.body, &result, context.clone());
        self.recur_target = previous;
        self.checking_function_body = enclosing_body;
        self.try_target = previous_try;
        let (body, found) = inferred?;
        let result_span = Self::block_result_span(&body).unwrap_or(span);
        self.expect(&result, &found, result_span, context)?;
        let body = self.view_closure_result(body, &result, &found)?;
        let parameter_types = params.iter().map(|param| param.ty.clone()).collect();
        let function = Type::Function(
            crate::types::ClosureKind::Fn,
            parameter_types,
            Box::new(result),
        );
        Ok((
            TypedExprKind::Fn(Rc::new(TypedLambda {
                params,
                body,
                wraps: None,
                kind: crate::types::ClosureKind::Fn,
                moved_capture: None,
                changed_capture: None,
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
        self.infer_block_with(block, None)
    }

    fn infer_block_expecting(
        &mut self,
        block: &Block,
        expected: &Type,
        context: MismatchContext,
    ) -> Result<(TypedBlock, Type), Diagnostic> {
        self.infer_block_with(block, Some((expected, context)))
    }

    fn infer_block_with(
        &mut self,
        block: &Block,
        expected: Option<(&Type, MismatchContext)>,
    ) -> Result<(TypedBlock, Type), Diagnostic> {
        let mut items = Vec::new();
        let mut result = Type::unit();
        let last = block.items.len().checked_sub(1);
        for (position, item) in block.items.iter().enumerate() {
            let (typed, item_type) = match (item, Some(position) == last, &expected) {
                (BodyItem::Expr(expr), true, Some((expected, context))) => {
                    let typed = self.infer_against(expr, expected, context.clone())?;
                    let ty = typed.ty.clone();
                    (TypedBodyItem::Expr(typed), ty)
                }
                _ => self.infer_body_item(item)?,
            };
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
        let (value, ty) = match &binder.annotation {
            Some(annotation) => {
                let ty = self.lower_type(annotation, &HashMap::new())?;
                let context = MismatchContext::Annotation {
                    annotation: annotation.span(),
                };
                (self.infer_against(value, &ty, context)?, ty)
            }
            None => {
                let value = self.infer(value)?;
                let ty = value.ty.clone();
                (value, ty)
            }
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

    fn infer_call(
        &mut self,
        callee: &Expr,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        self.infer_call_expecting(callee, arguments, None, span)
    }

    fn infer_call_expecting(
        &mut self,
        callee: &Expr,
        arguments: &[Expr],
        expected: Option<&Type>,
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
        let boxed = self.box_object(expected, &result);
        if boxed.is_none() {
            self.expect_call_result(expected, &result, span)?;
        }
        let (callee, typed_arguments) =
            self.infer_call_arguments(callee, &parameters, arguments)?;
        let call = TypedExpr {
            kind: TypedExprKind::Call(Box::new(callee), typed_arguments),
            ty: result,
            span,
        };
        self.finish_call(call, boxed, expected, span)
    }

    fn infer_call_arguments(
        &mut self,
        callee: TypedExpr,
        parameters: &[Type],
        arguments: &[Expr],
    ) -> Result<(TypedExpr, Vec<TypedArgument>), Diagnostic> {
        let shared = SharedVariables::of(parameters, &self.unifier, &self.traits);
        let mut typed_arguments =
            self.infer_arguments(Some(&callee), parameters, arguments, &shared)?;
        self.pass_shared_arguments(Some(&callee), &shared, &mut typed_arguments)?;
        Ok((callee, typed_arguments))
    }

    fn finish_call(
        &mut self,
        call: TypedExpr,
        boxed: Option<BindingId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        match (boxed, expected) {
            (Some(trait_binding), Some(expected)) => {
                self.coerce_box(call, trait_binding, expected, span)
            }
            _ => Ok(call),
        }
    }

    fn box_object(&self, expected: Option<&Type>, result: &Type) -> Option<BindingId> {
        let expected = expected?;
        let Type::Named(expected_box, expected_arguments) = self.unifier.resolve(expected) else {
            return None;
        };
        let [Type::TraitObject(trait_binding)] = expected_arguments.as_slice() else {
            return None;
        };
        let Type::Named(result_box, result_arguments) = self.unifier.resolve(result) else {
            return None;
        };
        if result_box != expected_box || result_arguments.len() != 1 {
            return None;
        }
        Some(*trait_binding)
    }

    fn coerce_box(
        &mut self,
        call: TypedExpr,
        trait_binding: BindingId,
        expected: &Type,
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        let Type::Named(_, arguments) = self.unifier.resolve(&call.ty) else {
            unreachable!("a boxed call has a box type")
        };
        let argument = arguments
            .first()
            .expect("a boxed call has one type argument");
        if matches!(argument, Type::TraitObject(object) if *object == trait_binding) {
            return Ok(TypedExpr {
                ty: expected.clone(),
                ..call
            });
        }
        let bound = Bound::plain(trait_binding);
        let satisfied = self.traits.satisfies(&bound, argument, &|_, _| false);
        if !satisfied {
            self.report_missing_trait(&bound, argument, span);
        }
        Ok(TypedExpr {
            kind: TypedExprKind::TraitObject(Box::new(call), trait_binding),
            ty: expected.clone(),
            span,
        })
    }

    fn expect_call_result(
        &mut self,
        expected: Option<&Type>,
        result: &Type,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let Some(expected) = expected else {
            return Ok(());
        };
        let resolved = self.unifier.resolve(result);
        let expected_resolved = self.unifier.resolve(expected);
        if resolved.has_vars()
            && (resolved.shares_head_with(&expected_resolved)
                || self.coerces(&expected_resolved, &resolved, span))
        {
            self.expect(expected, result, span, MismatchContext::Plain)?;
        }
        Ok(())
    }

    fn infer_recur(&mut self, arguments: &[Expr], span: Span) -> Result<TypedExpr, Diagnostic> {
        let target = self
            .recur_target
            .clone()
            .expect("expansion checks that `recur` is inside a function");
        let shared = SharedVariables::of(&target.parameters, &self.unifier, &self.traits);
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
            if operand.is_unknown()
                && let Some(reported) = self.report_unknown_recursive_try(&value, span)
            {
                return reported;
            }
            return Err(TypeProblems::try_needs_value(
                &self.describe(&operand),
                span,
            ));
        };
        let expected = self.try_return_type(&source);
        let conversion = if self
            .unifier
            .unify(&expected, &target.returns, span)
            .is_err()
        {
            self.convert_try_error(&source, &target.returns, span)?;
            TryErrorConversion::Converted
        } else {
            TryErrorConversion::Identity
        };
        Ok(TypedExpr {
            kind: TypedExprKind::Try {
                value: Box::new(value),
                conversion,
            },
            ty: source.value,
            span,
        })
    }

    fn report_unknown_recursive_try(
        &mut self,
        value: &TypedExpr,
        span: Span,
    ) -> Option<Result<TypedExpr, Diagnostic>> {
        let called = self.recursive_try_callee(value)?;
        let group = self.recursive_group.iter().copied().min()?;
        if !self.reported_unknown_try_groups.insert(group) {
            return Some(Ok(TypedExpr {
                kind: TypedExprKind::Try {
                    value: Box::new(value.clone()),
                    conversion: TryErrorConversion::Identity,
                },
                ty: self.unifier.fresh(),
                span,
            }));
        }
        let called_name = self.bindings.get(called).name();
        let names: Vec<String> = self
            .recursive_group
            .iter()
            .map(|binding| self.bindings.get(*binding).name().to_string())
            .collect();
        let hides_prelude = self.prelude_value_names.contains(called_name);
        Some(Err(TypeProblems::try_needs_unknown_recursive(
            called_name,
            &names,
            hides_prelude,
            span,
        )))
    }

    fn recursive_try_callee(&self, value: &TypedExpr) -> Option<BindingId> {
        let TypedExprKind::Call(callee, _) = &value.kind else {
            return None;
        };
        let TypedExprKind::Leaf(TypedLeaf::Global(binding)) = &callee.kind else {
            return None;
        };
        self.recursive_group.contains(binding).then_some(*binding)
    }

    fn convert_try_error(
        &mut self,
        source: &TrySource,
        returns: &Type,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let returns = self.unifier.resolve(returns);
        let Some(target) = self.try_source(&returns) else {
            return Err(self.try_target_problem(source, &returns, span));
        };
        if source.kind != TrySourceKind::Result || target.kind != TrySourceKind::Result {
            return Err(self.try_target_problem(source, &returns, span));
        }
        let source_error = source
            .error
            .clone()
            .expect("a result source carries an error type");
        let target_error = target.error.expect("a result target carries an error type");
        let conversion = self.conversion_variant(&target_error, &source_error, span)?;
        self.record_conversion(conversion);
        Ok(())
    }

    fn try_target_problem(&self, source: &TrySource, returns: &Type, span: Span) -> Diagnostic {
        let source_name = self.bindings.get(source.binding).name();
        TypeProblems::try_cannot_return(
            &source.kind.needed(source_name),
            &self.describe(returns),
            span,
        )
    }

    fn conversion_variant(
        &mut self,
        error: &Type,
        source: &Type,
        span: Span,
    ) -> Result<TryConversion, Diagnostic> {
        let Type::Named(binding, _) = error else {
            return Err(self.no_conversion(error, source, span));
        };
        let Some(definition) = self.enums.get(*binding) else {
            return Err(self.no_conversion(error, source, span));
        };
        if definition.rust.is_some() || !definition.params.is_empty() {
            return Err(self.no_conversion(error, source, span));
        }
        let candidates: Vec<(usize, Rc<str>, Type)> = definition
            .variants
            .iter()
            .enumerate()
            .filter_map(|(variant, candidate)| {
                let VariantPayload::Positional(payloads) = &candidate.payload else {
                    return None;
                };
                let [payload] = payloads.as_slice() else {
                    return None;
                };
                (!self.is_box(payload) && Self::could_unify(payload, source))
                    .then(|| (variant, Rc::clone(&candidate.name), payload.clone()))
            })
            .collect();
        if candidates.len() > 1 {
            let variants: Vec<(String, String)> = candidates
                .iter()
                .map(|(_, name, payload)| (name.to_string(), self.describe(payload)))
                .collect();
            return Err(TypeProblems::try_ambiguous_conversion(
                &self.describe(source),
                &self.describe(error),
                &variants,
                span,
            ));
        }
        let Some((variant, _, payload)) = candidates.first() else {
            return Err(self.no_conversion(error, source, span));
        };
        self.unifier
            .unify(payload, source, span)
            .map_err(|_| self.no_conversion(error, source, span))?;
        Ok(TryConversion {
            enum_binding: *binding,
            variant: *variant,
            payload: payload.clone(),
        })
    }

    fn no_conversion(&self, error: &Type, source: &Type, span: Span) -> Diagnostic {
        TypeProblems::try_no_conversion(&self.describe(source), &self.describe(error), span)
    }

    fn could_unify(left: &Type, right: &Type) -> bool {
        if !right.has_vars() {
            return left == right;
        }
        match (left, right) {
            (_, Type::Var(_)) => true,
            (Type::Named(left, left_parts), Type::Named(right, right_parts)) => {
                left == right
                    && left_parts.len() == right_parts.len()
                    && left_parts
                        .iter()
                        .zip(right_parts)
                        .all(|(left, right)| Self::could_unify(left, right))
            }
            _ => false,
        }
    }

    fn record_conversion(&mut self, conversion: TryConversion) {
        let known = self.conversions.iter().any(|known| {
            known.enum_binding == conversion.enum_binding && known.error == conversion.payload
        });
        if !known {
            self.conversions.push(TypedConversion {
                enum_binding: conversion.enum_binding,
                variant: conversion.variant,
                error: conversion.payload,
            });
        }
    }

    fn try_source(&self, operand: &Type) -> Option<TrySource> {
        let Type::Named(binding, arguments) = operand else {
            return None;
        };
        let definition = self.enums.get(*binding)?;
        if definition.is_result_shaped() && arguments.len() == 2 {
            return Some(TrySource {
                kind: TrySourceKind::Result,
                binding: *binding,
                value: arguments[0].clone(),
                error: Some(arguments[1].clone()),
            });
        }
        if definition.is_option_shaped() && arguments.len() == 1 {
            return Some(TrySource {
                kind: TrySourceKind::Option,
                binding: *binding,
                value: arguments[0].clone(),
                error: None,
            });
        }
        None
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
            let site = ArgumentSite { callee, position };
            let value = self.infer_argument(parameter, argument, &site)?;
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
        site: &ArgumentSite<'_>,
    ) -> Result<TypedExpr, Diagnostic> {
        let span = argument.span();
        let ExprKind::Fn(lambda) = argument.kind() else {
            let expected = parameter.clone().without_reference();
            let value = if self.contains_box_object(&expected) {
                self.infer_expecting(argument, &expected, &MismatchContext::Plain)?
            } else {
                self.infer(argument)?
            };
            let value = self.wrap_function_value(parameter, value)?;
            return self.iterate_collection(parameter, value, site);
        };
        match self.unifier.shallow(parameter) {
            Type::Function(_, parameters, result) if parameters.len() == lambda.params.len() => {
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

    fn iterate_collection(
        &mut self,
        parameter: &Type,
        value: TypedExpr,
        site: &ArgumentSite<'_>,
    ) -> Result<TypedExpr, Diagnostic> {
        let expected = self.unifier.resolve(parameter);
        if value.is_written_form() || !Pipelines::new(&self.rust_types).is_pipeline(&expected) {
            return Ok(value);
        }
        let Some((item, collection_type)) = self.collection_item(&value.ty) else {
            return Ok(value);
        };
        let (template, item) = self.iteration(&expected, item);
        let result = self.iterator_type(item);
        let parameters = vec![Type::shared(collection_type)];
        let span = value.span;
        let call =
            self.synthetic_call_with_value(template, parameters, result, value, &[], span)?;
        self.expect(&expected, &call.ty, span, MismatchContext::Plain)
            .map_err(|diagnostic| self.explain_borrowed_items(site, &call.ty, diagnostic))?;
        Ok(call)
    }

    fn iteration(&self, expected: &Type, item: Type) -> (&'static str, Type) {
        match item {
            Type::Ref(_, element) if self.expects_copied_items(expected, &element) => {
                ("{0}.iter().copied()", *element)
            }
            item => ("{0}.iter()", item),
        }
    }

    fn expects_copied_items(&self, expected: &Type, element: &Type) -> bool {
        let Type::Named(_, arguments) = expected else {
            return false;
        };
        let [item] = arguments.as_slice() else {
            return false;
        };
        match self.unifier.shallow(item) {
            Type::Var(var) => self.allows_only_copy_values(var),
            item => self.is_copy_element(&item, element),
        }
    }

    fn is_copy_element(&self, item: &Type, element: &Type) -> bool {
        let item = self.unifier.resolve(item).without_written_lifetimes();
        self.copy_rules().is_copy(&item) && self.element_is(element, &item)
    }

    fn element_is(&self, element: &Type, item: &Type) -> bool {
        match (self.unifier.shallow(element), item) {
            (Type::Var(var), Type::Primitive(primitive)) => self
                .unifier
                .literal_kind(var)
                .is_some_and(|kind| kind.accepts(*primitive)),
            (element, item) => &self.unifier.resolve(&element).without_written_lifetimes() == item,
        }
    }

    fn collection_item(&self, ty: &Type) -> Option<(Type, Type)> {
        let resolved = self.unifier.resolve(ty);
        let collection_type = match &resolved {
            Type::Ref(_, inner) | Type::Mut(_, inner) => self.unifier.resolve(inner),
            _ => resolved.clone(),
        };
        if let Type::Named(binding, arguments) = &collection_type
            && self.rust_types.is_slice(*binding)
        {
            let [element] = arguments.as_slice() else {
                return None;
            };
            return Some((
                Type::shared(self.unifier.resolve(element)),
                collection_type.clone(),
            ));
        }
        let collection = Collection::of(&collection_type, &self.rust_types)?;
        let item = if collection.is_map() {
            let key = self.unifier.resolve(collection.key());
            let entry = self.unifier.resolve(collection.value());
            self.pair_type(Type::shared(key), Type::shared(entry))
        } else {
            Type::shared(self.unifier.resolve(collection.value()))
        };
        Some((item, collection.ty().clone()))
    }

    fn contains_box_object(&self, ty: &Type) -> bool {
        match self.unifier.resolve(ty) {
            Type::TraitObject(_) => true,
            Type::Named(binding, arguments) if self.rust_types.is_box(binding) => {
                matches!(arguments.as_slice(), [Type::TraitObject(_)])
            }
            Type::Named(_, arguments) => arguments
                .iter()
                .any(|argument| self.contains_box_object(argument)),
            Type::Ref(_, inner) | Type::Mut(_, inner) => self.contains_box_object(&inner),
            Type::Function(_, parameters, result) => {
                self.function_contains_box_object(&parameters, &result)
            }
            Type::Var(_) | Type::Generic(_) | Type::Primitive(_) => false,
        }
    }

    fn function_contains_box_object(&self, parameters: &[Type], result: &Type) -> bool {
        parameters
            .iter()
            .any(|parameter| self.contains_box_object(parameter))
            || self.contains_box_object(result)
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
        let (Type::Function(_, expected, expected_result), Type::Function(_, found, found_result)) = (
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
            Type::Function(_, parameters, result) => Ok((parameters, *result)),
            Type::Var(var) if self.unifier.literal_kind(var).is_none() => {
                let parameters: Vec<Type> =
                    (0..argument_count).map(|_| self.unifier.fresh()).collect();
                let result = self.unifier.fresh();
                let shape = Type::Function(
                    crate::types::ClosureKind::Fn,
                    parameters.clone(),
                    Box::new(result.clone()),
                );
                self.expect(&shape, &callee.ty, callee.span, MismatchContext::Plain)?;
                Ok((parameters, result))
            }
            other => Err(TypeProblems::not_a_function(
                &self.describe(&other),
                callee.span,
            )),
        }
    }

    pub(super) fn pass_call_argument(
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
            && let Type::Ref(_, referent) | Type::Mut(_, referent) = &value.ty
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
        let site = ArgumentSite { callee, position };
        self.pass_argument(parameter, value)
            .map_err(|diagnostic| self.explain_literal_default(call, position, diagnostic))
            .map_err(|diagnostic| self.explain_borrowed_items(&site, &value.ty, diagnostic))
    }

    fn explain_literal_default(
        &self,
        callee: BindingId,
        position: usize,
        diagnostic: Diagnostic,
    ) -> Diagnostic {
        if *diagnostic.code() != DiagnosticCode::MISMATCHED_TYPES {
            return diagnostic;
        }
        let Some(default) = self.literal_defaults.get(callee, position) else {
            return diagnostic;
        };
        let function = self.bindings.get(callee).name();
        diagnostic
            .with_note(TypeProblems::literal_default_note(
                &default.parameter,
                &default.default,
                function,
                &default.calls,
            ))
            .with_help(TypeProblems::literal_default_help(
                function,
                &default.signature,
            ))
    }

    pub(super) fn parameter_is_written(&self, callee: BindingId, position: usize) -> bool {
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
            (Type::Ref(_, inner), found) if !found.is_reference() => {
                if let Some(help) = self.string_as_str_help(inner, found, value) {
                    return Err(TypeProblems::mismatch(
                        &self.describe(inner),
                        &self.describe(found),
                        value.span,
                        MismatchContext::Plain,
                    )
                    .with_help(help));
                }
                self.pass(inner, found, value.span, Borrow::Shared)
            }
            (Type::Mut(_, inner), _) => self.pass_mut_argument(inner, &parameter, &argument, value),
            (Type::Var(var), Type::Ref(_, _)) if self.referent_unknown(&argument) => {
                Ok(self.defer_borrow(*var, &argument, value.span))
            }
            _ => self.pass_types(&parameter, &argument, value.span),
        }
    }

    fn string_as_str_help(
        &self,
        expected: &Type,
        found: &Type,
        value: &TypedExpr,
    ) -> Option<String> {
        if !matches!(expected, Type::Primitive(PrimitiveType::Str))
            || !matches!(found, Type::Primitive(PrimitiveType::String))
        {
            return None;
        }
        let place = self.local_place(value)?;
        Some(format!("borrow it as a `str` with `(as-str {place})`"))
    }

    fn pass_mut_argument(
        &mut self,
        inner: &Type,
        parameter: &Type,
        argument: &Type,
        value: &TypedExpr,
    ) -> Result<Borrow, Diagnostic> {
        let place = self.place_type(value);
        if value.is_place() && !place.is_reference() {
            return self.pass(inner, &place, value.span, Borrow::Mutable);
        }
        if !value.is_place() && !argument.is_reference() {
            return Err(TypeProblems::mut_borrow_of_copy(
                value.span,
                self.field_place(value),
            ));
        }
        self.pass_types(parameter, argument, value.span)
    }

    fn slice_argument(&mut self, parameter: &Type, argument: &Type, span: Span) -> Type {
        let found = self.unifier.resolve(argument);
        match (parameter, &found) {
            (Type::Ref(_, expected), Type::Ref(_, referent)) => {
                Type::shared(self.vector_before_slice(expected, referent, span))
            }
            (Type::Ref(_, expected), referent) if !referent.is_reference() => {
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
        self.unifier.literal_kind(var).is_some() || self.allows_only_copy_values(var)
    }

    fn allows_only_copy_values(&self, var: TypeVar) -> bool {
        self.unifier.bounds(var).iter().any(|bound| {
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
            (expected, Type::Ref(_, _)) if Self::is_known_value(expected) => {
                let levels = Self::reference_depth(argument);
                let referent = Self::innermost_referent(argument).clone();
                self.expect(expected, &referent, span, MismatchContext::Plain)?;
                Ok(self.borrow_of_referent(&referent, levels))
            }
            (Type::Ref(_, _), Type::Ref(_, _)) if self.collapses(parameter, argument) => {
                let parameter = self.unifier.resolve(parameter);
                let extra = Self::reference_depth(argument) - Self::reference_depth(&parameter);
                let stripped = Self::without_references(argument, extra);
                self.pass(&parameter, &stripped, span, Borrow::Collapse(extra))
            }
            (Type::Var(var), Type::Ref(_, _)) => self.pass_to_variable(*var, argument, span),
            (Type::Mut(_, _), Type::Mut(_, _)) => {
                self.pass(parameter, argument, span, Borrow::Reborrow)
            }
            (Type::Ref(_, _), Type::Mut(_, _)) if self.coerces(parameter, argument, span) => {
                self.pass(parameter, argument, span, Borrow::Reborrow)
            }
            (expected, found) => self.pass_value(expected, found, span),
        }
    }

    fn pass_value(
        &mut self,
        expected: &Type,
        found: &Type,
        span: Span,
    ) -> Result<Borrow, Diagnostic> {
        if expected.is_unsized() && matches!(found, Type::Var(_)) {
            let borrowed = Type::shared(expected.clone());
            return self.pass(&borrowed, found, span, Borrow::None);
        }
        self.pass(expected, found, span, Borrow::None)
    }

    fn collapses(&self, parameter: &Type, argument: &Type) -> bool {
        let parameter = self.unifier.resolve(parameter);
        Self::reference_depth(argument) > Self::reference_depth(&parameter)
    }

    fn reference_depth(value: &Type) -> usize {
        match value {
            Type::Ref(_, inner) => 1 + Self::reference_depth(inner),
            _ => 0,
        }
    }

    fn innermost_referent(value: &Type) -> &Type {
        match value {
            Type::Ref(_, inner) => Self::innermost_referent(inner),
            _ => value,
        }
    }

    fn without_references(value: &Type, levels: usize) -> Type {
        match value {
            Type::Ref(_, inner) if levels > 0 => Self::without_references(inner, levels - 1),
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
        let label = self.single_field_readers.get(&binding)?;
        let [argument] = arguments.as_slice() else {
            return None;
        };
        let place = self.local_place(&argument.value)?;
        Some(format!("({}{label} {place})", Keyword::MARKER))
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
        while let Type::Ref(_, referent) = passed
            && self.dereferences(var, passed, referent)
        {
            levels += 1;
            passed = referent;
        }
        if levels == 0
            && let Type::Ref(_, referent) = argument
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
        let typed_fields = self.check_fields(binding, &arguments, fields)?;
        let declared = self.instantiated_fields(binding, &arguments);
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

    fn infer_assoc(
        &mut self,
        value: &Expr,
        keys: &[Expr],
        item: &Expr,
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        let value = self.infer(value)?;
        let value = self.moved_field_base(value);
        let resolved = self.unifier.resolve(&value.ty);
        if let Some(collection) =
            Collection::of(&resolved, &self.rust_types).filter(Collection::is_map)
        {
            return self.infer_map_assoc(value, resolved, collection, keys, item, span);
        }
        self.infer_vector_assoc(value, keys, item, span)
    }

    fn infer_map_assoc(
        &mut self,
        value: TypedExpr,
        resolved: Type,
        collection: Collection,
        keys: &[Expr],
        item: &Expr,
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        if keys.len() != 1 {
            return Err(TypeProblems::wrong_argument_count(1, keys.len(), span));
        }
        let key = self.infer(&keys[0])?;
        self.expect(collection.key(), &key.ty, key.span, MismatchContext::Plain)?;
        self.check_map_key(&collection, key.span);
        let item = self.infer(item)?;
        self.expect(
            collection.value(),
            &item.ty,
            item.span,
            MismatchContext::Plain,
        )?;
        Ok(TypedExpr {
            kind: TypedExprKind::Assoc(Box::new(value), vec![key], Box::new(item)),
            ty: resolved,
            span,
        })
    }

    fn infer_vector_assoc(
        &mut self,
        value: TypedExpr,
        keys: &[Expr],
        item: &Expr,
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        let element = self.assoc_element(keys.len(), &value)?;
        let typed_keys = self.assoc_keys(keys)?;
        let item = self.infer(item)?;
        self.expect(&element, &item.ty, item.span, MismatchContext::Plain)?;
        let ty = self.unifier.resolve(&value.ty);
        Ok(TypedExpr {
            kind: TypedExprKind::Assoc(Box::new(value), typed_keys, Box::new(item)),
            ty,
            span,
        })
    }

    fn infer_map_at(
        &mut self,
        operation: MapOperation,
        value: &Expr,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        let value = self.infer(value)?;
        if operation == MapOperation::Iter {
            return self.infer_iter(value, arguments, span);
        }
        let collection = self.map_collection(&value)?;
        self.check_map_key(&collection, value.span);
        match operation {
            MapOperation::Lookup => self.infer_map_lookup(value, collection, arguments, span),
            MapOperation::Update => self.infer_map_update(value, collection, arguments, span),
            _ => self.infer_map_iterator_operation(operation, value, collection, arguments, span),
        }
    }

    fn infer_map_iterator_operation(
        &mut self,
        operation: MapOperation,
        value: TypedExpr,
        collection: Collection,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        let iterator = match operation {
            MapOperation::Keys => MapIterator::Keys,
            MapOperation::Vals => MapIterator::Vals,
            _ => MapIterator::Entries,
        };
        self.infer_map_iterator(value, collection, iterator, arguments, span)
    }

    fn infer_iter(
        &mut self,
        value: TypedExpr,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        if !arguments.is_empty() {
            return Err(TypeProblems::wrong_argument_count(0, arguments.len(), span));
        }
        let Some((item, collection_type)) = self.collection_item(&value.ty) else {
            let resolved = self.unifier.resolve(&value.ty);
            if resolved.is_unknown() && !self.is_literal(&resolved) {
                return self.infer_iter_slice(value, span);
            }
            return Err(TypeProblems::iter_needs_collection(
                &self.describe(&resolved),
                value.span,
            ));
        };
        if let Some(collection) = Collection::of(&collection_type, &self.rust_types) {
            if collection.kind() == CollectionKind::Vector {
                return self.infer_iter_slice(value, span);
            }
            self.check_map_key(&collection, value.span);
        }
        let result = self.iterator_type(item);
        let parameters = vec![Type::shared(collection_type)];
        self.synthetic_call_with_value("{0}.iter()", parameters, result, value, arguments, span)
    }

    fn infer_iter_slice(&mut self, value: TypedExpr, span: Span) -> Result<TypedExpr, Diagnostic> {
        let element = self.unifier.fresh();
        let slice = self
            .rust_types
            .binding_with_template(RustType::SLICE_TEMPLATE)
            .expect("the prelude defines a slice type");
        let parameter = Type::shared(Type::Named(slice, vec![element.clone()]));
        let result = self.iterator_type(Type::shared(element));
        self.synthetic_call_with_value("{0}.iter()", vec![parameter], result, value, &[], span)
    }

    fn map_collection(&self, value: &TypedExpr) -> Result<Collection, Diagnostic> {
        let resolved = self.unifier.resolve(&value.ty);
        Collection::of_referent(&resolved, &self.rust_types)
            .filter(Collection::is_map)
            .ok_or_else(|| {
                TypeProblems::mismatch(
                    "a map",
                    &self.display().show(&resolved),
                    value.span,
                    MismatchContext::Plain,
                )
            })
    }

    fn check_map_key(&mut self, collection: &Collection, span: Span) {
        let paths: &[&str] = if collection.kind() == CollectionKind::SortedMap {
            &["Ord"]
        } else {
            &["std::hash::Hash", "Eq"]
        };
        let key = self.unifier.resolve(collection.key());
        if key.is_unknown() {
            return;
        }
        for path in paths {
            let binding = self
                .traits
                .with_paths(&[path])
                .first()
                .copied()
                .expect("the prelude defines the map key traits");
            let bound = Bound::plain(binding);
            if !self.traits.satisfies(&bound, &key, &|_, _| false) {
                self.report_missing_trait(&bound, &key, span);
            }
        }
    }

    fn infer_map_lookup(
        &mut self,
        value: TypedExpr,
        collection: Collection,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        if arguments.len() != 1 {
            return Err(TypeProblems::wrong_argument_count(1, arguments.len(), span));
        }
        let query_type = self.unifier.fresh();
        let parameters = vec![
            Type::shared(collection.ty().clone()),
            Type::shared(query_type),
        ];
        let result = self.option_type(collection.value().clone());
        let call = self.synthetic_call_with_value(
            "{0}.get({1})",
            parameters,
            result,
            value,
            arguments,
            span,
        )?;
        let query = match &call.kind {
            TypedExprKind::Call(_, arguments) => {
                arguments.get(1).map(|argument| argument.value.ty.clone())
            }
            _ => None,
        };
        if let Some(query) = query {
            self.check_borrow(&collection, &query, arguments[0].span());
        }
        Ok(call)
    }

    fn check_borrow(&mut self, collection: &Collection, query: &Type, span: Span) {
        let key = self.unifier.resolve(collection.key());
        let query = self.unifier.resolve(query).without_reference();
        if key.is_unknown() || query.is_unknown() {
            return;
        }
        let binding = self
            .traits
            .with_paths(&["std::borrow::Borrow"])
            .first()
            .copied()
            .expect("the prelude defines borrow");
        let bound = Bound {
            binding,
            argument: Some(query),
        };
        if !self.traits.satisfies(&bound, &key, &|_, _| false) {
            self.report_missing_trait(&bound, &key, span);
        }
    }

    fn infer_map_iterator(
        &mut self,
        value: TypedExpr,
        collection: Collection,
        iterator: MapIterator,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        if !arguments.is_empty() {
            return Err(TypeProblems::wrong_argument_count(0, arguments.len(), span));
        }
        let result = match iterator {
            MapIterator::Keys => self.iterator_type(Type::shared(collection.key().clone())),
            MapIterator::Vals => self.iterator_type(Type::shared(collection.value().clone())),
            MapIterator::Entries => self.iterator_type(self.pair_type(
                Type::shared(collection.key().clone()),
                Type::shared(collection.value().clone()),
            )),
        };
        let template = match iterator {
            MapIterator::Keys => "{0}.keys()",
            MapIterator::Vals => "{0}.values()",
            MapIterator::Entries => "{0}.iter()",
        };
        let parameters = vec![Type::shared(collection.ty().clone())];
        self.synthetic_call_with_value(template, parameters, result, value, arguments, span)
    }

    fn infer_map_update(
        &mut self,
        value: TypedExpr,
        collection: Collection,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        if arguments.len() != 3 {
            return Err(TypeProblems::wrong_argument_count(3, arguments.len(), span));
        }
        let value_type = collection.value().clone();
        let function_type = Type::Function(
            crate::types::ClosureKind::Fn,
            vec![value_type.clone()],
            Box::new(value_type.clone()),
        );
        let parameters = vec![
            collection.ty().clone(),
            collection.key().clone(),
            function_type,
            value_type,
        ];
        let result = collection.ty().clone();
        self.synthetic_call_with_value("@map-update", parameters, result, value, arguments, span)
    }

    fn infer_set_at(
        &mut self,
        value: &Expr,
        keys: &[Expr],
        item: &Expr,
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        let value = self.infer(value)?;
        let resolved = self.unifier.resolve(&value.ty);
        let Some(collection) = Collection::of_referent(&resolved, &self.rust_types) else {
            return Err(TypeProblems::mismatch(
                "a collection",
                &self.display().show(&resolved),
                value.span,
                MismatchContext::Plain,
            ));
        };
        if collection.is_map() {
            return self.infer_set_at_map(value, collection, keys, item, span);
        }
        let depth = keys.len();
        let item_type = self.assoc_element(depth, &value)?;
        let mut parameters = vec![Type::mutable(collection.ty().clone())];
        parameters.extend((0..depth).map(|_| Type::Primitive(PrimitiveType::Usize)));
        parameters.push(item_type);
        let mut template = "{0}".to_string();
        for index in 1..=depth {
            template.push_str(&format!("[{{{index}}}]"));
        }
        template.push_str(&format!(" = {{{}}}", depth + 1));
        let mut arguments = Vec::with_capacity(keys.len() + 1);
        arguments.extend(keys.iter().cloned());
        arguments.push(item.clone());
        self.synthetic_call_with_value(&template, parameters, Type::unit(), value, &arguments, span)
    }

    fn infer_set_at_map(
        &mut self,
        value: TypedExpr,
        collection: Collection,
        keys: &[Expr],
        item: &Expr,
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        self.check_map_key(&collection, value.span);
        if keys.len() != 1 {
            return Err(TypeProblems::wrong_argument_count(1, keys.len(), span));
        }
        let query_type = self.unifier.fresh();
        let parameters = vec![
            Type::mutable(collection.ty().clone()),
            Type::shared(query_type.clone()),
            collection.value().clone(),
        ];
        let mut arguments = Vec::with_capacity(keys.len() + 1);
        arguments.extend(keys.iter().cloned());
        arguments.push(item.clone());
        let call = self.synthetic_call_with_value(
            "*{0}.get_mut({1}).unwrap() = {2}",
            parameters,
            Type::unit(),
            value,
            &arguments,
            span,
        )?;
        let query = self.unifier.resolve(&query_type);
        self.check_borrow(&collection, &query, keys[0].span());
        Ok(call)
    }

    fn synthetic_call_with_value(
        &mut self,
        template: &str,
        parameters: Vec<Type>,
        result: Type,
        value: TypedExpr,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        let mut typed_arguments = Vec::with_capacity(parameters.len());
        let borrow = self.pass_call_argument(None, 0, &parameters[0], &value)?;
        typed_arguments.push(TypedArgument { value, borrow });
        for (position, argument) in arguments.iter().enumerate() {
            let index = position + 1;
            let parameter = &parameters[index];
            let site = ArgumentSite {
                callee: None,
                position: index,
            };
            let value = self.infer_argument(parameter, argument, &site)?;
            let borrow = self.pass_call_argument(None, index, parameter, &value)?;
            typed_arguments.push(TypedArgument { value, borrow });
        }
        let parameters: Vec<Type> = parameters
            .iter()
            .map(|parameter| self.unifier.resolve(parameter))
            .collect();
        let result = self.unifier.resolve(&result);
        let callee = self.rust_template(template, parameters, result.clone(), span);
        Ok(TypedExpr {
            kind: TypedExprKind::Call(Box::new(callee), typed_arguments),
            ty: result,
            span,
        })
    }

    fn rust_template(
        &self,
        template: &str,
        parameters: Vec<Type>,
        result: Type,
        span: Span,
    ) -> TypedExpr {
        let template = RustTemplate {
            template: Rc::from(template),
            type_expr: TypeExpr::new(TypeExprKind::Hole, span),
        };
        TypedExpr {
            kind: TypedExprKind::Leaf(TypedLeaf::Rust(Rc::new(template))),
            ty: Type::Function(crate::types::ClosureKind::Fn, parameters, Box::new(result)),
            span,
        }
    }

    fn option_type(&self, value: Type) -> Type {
        let option = self
            .rust_types
            .option_binding()
            .expect("the prelude defines an option type");
        Type::Named(option, vec![Type::shared(value)])
    }

    fn iterator_type(&self, item: Type) -> Type {
        let iterator = self
            .rust_types
            .iterator_binding()
            .expect("the prelude defines an iterator type");
        Type::Named(iterator, vec![item])
    }

    fn pair_type(&self, first: Type, second: Type) -> Type {
        let pair = self
            .rust_types
            .pair_binding()
            .expect("the prelude defines a pair type");
        Type::Named(pair, vec![first, second])
    }

    fn assoc_element(&mut self, depth: usize, value: &TypedExpr) -> Result<Type, Diagnostic> {
        let element = self.unifier.fresh();
        let expected = self.nested_vector_type(depth, &element);
        self.expect(&expected, &value.ty, value.span, MismatchContext::Plain)
            .map_err(|error| self.point_struct_base_to_fields(error, &value.ty))?;
        Ok(element)
    }

    fn point_struct_base_to_fields(&self, error: Diagnostic, base: &Type) -> Diagnostic {
        if self.names_struct(base) {
            error.with_help(TypeProblems::struct_base_help())
        } else {
            error
        }
    }

    fn names_struct(&self, ty: &Type) -> bool {
        matches!(self.unifier.shallow(ty), Type::Named(binding, _) if self.structs.get(binding).is_some())
    }

    fn assoc_keys(&mut self, keys: &[Expr]) -> Result<Vec<TypedExpr>, Diagnostic> {
        let mut typed_keys = Vec::new();
        for key in keys {
            let typed = self.infer(key)?;
            self.expect(
                &Type::Primitive(PrimitiveType::Usize),
                &typed.ty,
                typed.span,
                MismatchContext::Plain,
            )?;
            typed_keys.push(typed);
        }
        Ok(typed_keys)
    }

    fn nested_vector_type(&self, depth: usize, element: &Type) -> Type {
        let vector = self
            .rust_types
            .binding_with_template(RustType::VECTOR_TEMPLATE)
            .expect("the prelude defines a vector type");
        (0..depth).fold(element.clone(), |ty, _| Type::Named(vector, vec![ty]))
    }

    fn infer_update(
        &mut self,
        base: &Expr,
        fields: &[(FieldLabel, Expr)],
        span: Span,
    ) -> Result<(TypedExprKind, Type), Diagnostic> {
        let base = self.infer(base)?;
        let base = self.moved_field_base(base);
        let (binding, arguments) = self.update_owner(&base, fields, span)?;
        let typed_fields = self.check_fields(binding, &arguments, fields)?;
        Ok((
            TypedExprKind::Struct(TypedStructExpr::Update(
                binding,
                Box::new(base),
                typed_fields,
            )),
            Type::Named(binding, arguments),
        ))
    }

    fn moved_field_base(&self, base: TypedExpr) -> TypedExpr {
        if self.reads_owned_field(&base) {
            TypedExpr {
                ty: self.place_type(&base),
                ..base
            }
        } else {
            base
        }
    }

    fn reads_owned_field(&self, base: &TypedExpr) -> bool {
        base.field_place().is_some_and(|place| {
            place.reads_field()
                && !matches!(
                    self.unifier.shallow(place.root_type),
                    Type::Ref(..) | Type::Mut(..)
                )
        })
    }

    fn update_owner(
        &mut self,
        base: &TypedExpr,
        fields: &[(FieldLabel, Expr)],
        span: Span,
    ) -> Result<(BindingId, Vec<Type>), Diagnostic> {
        let owner = self.unifier.shallow(&base.ty);
        match owner {
            Type::Named(binding, arguments) if self.structs.get(binding).is_some() => {
                Ok((binding, arguments))
            }
            Type::Var(_) => self.update_owner_from_fields(&owner, fields, span),
            other => Err(TypeProblems::no_fields(
                &self.display().show(&other),
                base.span,
            )),
        }
    }

    fn update_owner_from_fields(
        &mut self,
        owner: &Type,
        fields: &[(FieldLabel, Expr)],
        span: Span,
    ) -> Result<(BindingId, Vec<Type>), Diagnostic> {
        let first = fields
            .first()
            .expect("the expander requires a field in an update");
        let candidates: Vec<BindingId> = self
            .structs
            .with_field(&first.0.name)
            .into_iter()
            .filter(|binding| {
                let definition = self
                    .structs
                    .get(*binding)
                    .expect("a struct with a field has a definition");
                fields
                    .iter()
                    .all(|(label, _)| definition.field(&label.name).is_some())
            })
            .collect();
        let [binding] = candidates.as_slice() else {
            return Err(TypeProblems::ambiguous_field(&first.0.name, first.0.span));
        };
        let arguments = self.fresh_arguments(*binding);
        let expected = Type::Named(*binding, arguments.clone());
        self.expect(&expected, owner, span, MismatchContext::Plain)?;
        Ok((*binding, arguments))
    }

    fn check_fields(
        &mut self,
        binding: BindingId,
        arguments: &[Type],
        fields: &[(FieldLabel, Expr)],
    ) -> Result<Vec<(FieldLabel, TypedExpr)>, Diagnostic> {
        let struct_name = self.bindings.get(binding).name().to_string();
        let declared = self.instantiated_fields(binding, arguments);
        let mut typed_fields: Vec<(FieldLabel, TypedExpr)> = Vec::new();
        for (label, value) in fields {
            if let Some((first, _)) = typed_fields
                .iter()
                .find(|(earlier, _)| earlier.name == label.name)
            {
                return Err(TypeProblems::repeated_field(
                    &struct_name,
                    label,
                    first.span,
                ));
            }
            let Some((_, field_type)) = declared.iter().find(|(name, _)| *name == label.name)
            else {
                let names: Vec<Rc<str>> = declared.iter().map(|(name, _)| name.clone()).collect();
                return Err(TypeProblems::unknown_field(
                    &struct_name,
                    label,
                    SimilarName::closest(&label.name, &names),
                ));
            };
            let typed = self.infer_expecting(value, field_type, &MismatchContext::Plain)?;
            self.expect(field_type, &typed.ty, typed.span, MismatchContext::Plain)?;
            typed_fields.push((label.clone(), typed));
        }
        Ok(typed_fields)
    }

    fn instantiated_fields(&self, binding: BindingId, arguments: &[Type]) -> Vec<(Rc<str>, Type)> {
        self.structs
            .get(binding)
            .map(|definition| definition.instantiated_fields(arguments))
            .unwrap_or_default()
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
                label,
                SimilarName::closest(&label.name, &names),
            ));
        };
        let ty = if self.reads_field_by_copy(owner, &label.name, &ty) {
            ty
        } else {
            Type::shared(ty)
        };
        Ok(TypedExpr {
            kind: TypedExprKind::Struct(TypedStructExpr::Field(Box::new(value), label.clone())),
            ty,
            span,
        })
    }

    fn reads_field_by_copy(&self, owner: BindingId, field: &str, field_type: &Type) -> bool {
        self.declares_function_field(owner, field) || self.copy_rules().is_small_copy(field_type)
    }

    fn declares_function_field(&self, owner: BindingId, field: &str) -> bool {
        self.structs.get(owner).is_some_and(|definition| {
            definition
                .fields
                .iter()
                .any(|(name, declared)| &**name == field && matches!(declared, Type::Function(..)))
        })
    }

    fn field_owner(
        &mut self,
        value: &TypedExpr,
        label: &FieldLabel,
    ) -> Result<(BindingId, Vec<Type>), Diagnostic> {
        let mut owner = self.unifier.shallow(&value.ty);
        while let Type::Ref(_, inner) | Type::Mut(_, inner) = owner {
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
        BindingArena, BindingInfo, BindingKind, Expr, ExprKind, FieldLabel, FieldSpelling, Leaf,
        Literal, ModuleId, PrimitiveType,
    };
    use crate::source::{SourceId, Span};
    use crate::types::typed::{TypedExpr, TypedExprKind, TypedLeaf, TypedStructExpr};
    use crate::types::{ClosureKind, Type, TypeVar};

    #[test]
    fn a_function_error_with_an_open_part_does_not_convert() {
        let payload = Type::Function(
            ClosureKind::Fn,
            vec![Type::Primitive(PrimitiveType::I64)],
            Box::new(Type::Primitive(PrimitiveType::I64)),
        );
        let error = Type::Function(
            ClosureKind::Fn,
            vec![Type::Primitive(PrimitiveType::I64)],
            Box::new(Type::Var(TypeVar::new(0))),
        );
        assert!(!TypeChecker::could_unify(&payload, &error));
    }

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
                        spelling: FieldSpelling::Bare,
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
