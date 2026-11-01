use rustc_hash::FxHashSet;

use super::checker::{Admissible, TypeChecker};
use super::literal_fit::LiteralFit;
use super::problems::TypeProblems;
use super::ty::{GenericId, Type, TypeVar};
use super::typed::{ClosureType, TypedDefinition, TypedExpr, TypedExprKind, TypedLeaf};
use super::unifier::LiteralKind;
use crate::core::{BindingId, Literal, PrimitiveType};
use crate::reader::Number;
use crate::source::Span;

pub struct Finalizer<'c> {
    checker: &'c mut TypeChecker,
    reported: FxHashSet<TypeVar>,
    earlier_errors: bool,
    closures_in_definition: Vec<Type>,
}

impl<'c> Finalizer<'c> {
    pub fn new(checker: &'c mut TypeChecker) -> Self {
        let earlier_errors = checker.diagnostics.has_errors();
        Self {
            checker,
            reported: FxHashSet::default(),
            earlier_errors,
            closures_in_definition: Vec::new(),
        }
    }

    pub fn definition(&mut self, definition: &mut TypedDefinition) {
        self.expr(&mut definition.value);
        self.finalize_type(&mut definition.scheme.body, definition.span);
        self.record_closure_types(definition.binding);
    }

    fn record_closure_types(&mut self, definition: BindingId) {
        let closures = self
            .closures_in_definition
            .drain(..)
            .map(|ty| ClosureType { definition, ty });
        self.checker.closure_types.extend(closures);
    }

    fn expr(&mut self, expr: &mut TypedExpr) {
        self.finalize_type(&mut expr.ty, expr.span);
        if let TypedExprKind::Leaf(TypedLeaf::Literal(Literal::Number(number))) = &expr.kind {
            self.check_range(number, &expr.ty.clone(), expr.span);
        }
        for child in expr.children_mut() {
            self.expr(child);
            self.note_closure(child);
        }
        for binder in expr.binders_mut() {
            self.finalize_type(&mut binder.ty, binder.span);
        }
    }

    fn note_closure(&mut self, expr: &TypedExpr) {
        if let TypedExprKind::Fn(_) = expr.kind {
            self.closures_in_definition.push(expr.ty.clone());
        }
    }

    fn finalize_type(&mut self, ty: &mut Type, span: Span) {
        for var in self.checker.free_vars(ty) {
            self.default_var(var, span);
        }
        *ty = self.checker.unifier.resolve(ty);
    }

    fn default_var(&mut self, var: TypeVar, span: Span) {
        let default = match self.checker.unifier.literal_kind(var) {
            Some(LiteralKind::Integer) => PrimitiveType::I64,
            Some(LiteralKind::Float) => PrimitiveType::F64,
            None => {
                if self.reported.insert(var) && !self.earlier_errors {
                    self.checker
                        .defer_missing_annotation(TypeProblems::cannot_infer(span));
                }
                return;
            }
        };
        let defaulted =
            self.checker
                .unifier
                .unify(&Type::Var(var), &Type::Primitive(default), span);
        debug_assert!(
            defaulted.is_ok(),
            "a literal variable accepts its default type"
        );
    }

    fn check_range(&mut self, number: &Number, ty: &Type, span: Span) {
        match ty {
            _ if !LiteralFit::is_representable(number.value()) => {
                let widest = PrimitiveType::F64.name();
                let diagnostic = TypeProblems::out_of_range(number.text(), widest, span);
                self.checker.diagnostics.push(diagnostic);
            }
            Type::Generic(generic) if !self.checker.literal_generics.contains_key(generic) => {
                self.check_generic_literal(number, *generic, span);
            }
            Type::Primitive(primitive) if !LiteralFit::fits(number.value(), ty) => {
                let diagnostic = TypeProblems::out_of_range(number.text(), primitive.name(), span);
                self.checker.diagnostics.push(diagnostic);
            }
            _ => {}
        }
    }

    fn check_generic_literal(&mut self, number: &Number, generic: GenericId, span: Span) {
        let display = self.checker.display();
        let parameter = display.show(&Type::Generic(generic));
        let diagnostic = match self.checker.admissible_types(generic) {
            Admissible::Unbounded => Some(TypeProblems::literal_of_any_type(
                number.text(),
                &parameter,
                span,
            )),
            Admissible::Open(types) | Admissible::Closed(types) => types
                .iter()
                .find(|candidate| !LiteralFit::fits(number.value(), candidate))
                .map(|misfit| {
                    let names: Vec<String> = types
                        .iter()
                        .map(|candidate| display.show(candidate))
                        .collect();
                    TypeProblems::literal_misfit(
                        number.text(),
                        &parameter,
                        &display.show(misfit),
                        &names,
                        span,
                    )
                }),
        };
        if let Some(diagnostic) = diagnostic {
            self.checker.diagnostics.push(diagnostic);
        }
    }
}
