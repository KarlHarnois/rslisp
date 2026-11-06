use std::rc::Rc;

use rustc_hash::FxHashSet;

use super::checker::{Admissible, TypeChecker};
use super::literal_fit::LiteralFit;
use super::problems::TypeProblems;
use super::ty::{GenericId, Type, TypeVar};
use super::type_text::TypeTextWriter;
use super::typed::{ClosureType, TypedDefinition, TypedExpr, TypedExprKind, TypedLeaf};
use super::unifier::LiteralKind;
use crate::core::{BindingId, Literal, PrimitiveType};
use crate::diagnostic::Diagnostic;
use crate::reader::Number;
use crate::source::Span;

pub struct Finalizer<'c> {
    checker: &'c mut TypeChecker,
    reported: FxHashSet<TypeVar>,
    earlier_errors: bool,
    closures_in_definition: Vec<Type>,
    type_text_operands: Vec<TypedExpr>,
    inside_operand: bool,
}

impl<'c> Finalizer<'c> {
    pub fn new(checker: &'c mut TypeChecker) -> Self {
        let earlier_errors = checker.diagnostics.has_errors();
        Self {
            checker,
            reported: FxHashSet::default(),
            earlier_errors,
            closures_in_definition: Vec::new(),
            type_text_operands: Vec::new(),
            inside_operand: false,
        }
    }

    pub fn definition(&mut self, definition: &mut TypedDefinition) {
        self.expr(&mut definition.value);
        self.finalize_type(&mut definition.scheme.body, definition.span);
        self.record_closure_types(definition.binding);
        if self.checker.holds_infer_forms {
            self.checker.checking.type_text_walks += 1;
            let writer = TypeTextWriter::new(definition.binding, &definition.scheme);
            self.write_type_texts(&mut definition.value, &writer);
            let operands = std::mem::take(&mut self.type_text_operands);
            self.checker
                .type_texts
                .keep_operands(definition.binding, operands);
        }
    }

    fn write_type_texts(&mut self, expr: &mut TypedExpr, writer: &TypeTextWriter) {
        if let TypedExprKind::Infer(operand) = &mut expr.kind {
            let text = writer.write(self.checker, operand);
            self.finalize_operand(operand, writer);
            self.type_text_operands.push(operand.as_ref().clone());
            expr.kind = TypedExprKind::Leaf(TypedLeaf::Literal(Literal::String(text)));
            return;
        }
        for child in expr.children_mut() {
            self.write_type_texts(child, writer);
        }
    }

    fn finalize_operand(&mut self, operand: &mut TypedExpr, writer: &TypeTextWriter) {
        self.inside_operand = true;
        self.expr(operand);
        self.inside_operand = false;
        self.write_type_texts(operand, writer);
        self.closures_in_definition.clear();
    }

    fn record_closure_types(&mut self, definition: BindingId) {
        let closures = self
            .closures_in_definition
            .drain(..)
            .map(|ty| ClosureType { definition, ty });
        self.checker.closure_types.extend(closures);
    }

    fn expr(&mut self, expr: &mut TypedExpr) {
        if let TypedExprKind::Call(callee, arguments) = &mut expr.kind {
            if self.checker.unifier.holds_open_literal(&callee.ty) {
                self.default_literals(&callee.ty, callee.span);
            }
            for argument in arguments {
                self.child(&mut argument.value);
            }
            self.finalize_type(&mut expr.ty, expr.span);
            self.child(callee);
        } else {
            self.children(expr);
            self.finalize_type(&mut expr.ty, expr.span);
        }
        if let TypedExprKind::Leaf(TypedLeaf::Literal(Literal::Number(number))) = &expr.kind {
            self.check_range(number, &expr.ty.clone(), expr.span);
        }
        self.finalize_wrapped_function(expr);
    }

    fn children(&mut self, expr: &mut TypedExpr) {
        for child in expr.children_mut() {
            self.child(child);
        }
        for binder in expr.binders_mut() {
            self.finalize_type(&mut binder.ty, binder.span);
        }
    }

    fn child(&mut self, expr: &mut TypedExpr) {
        self.expr(expr);
        self.note_closure(expr);
    }

    fn default_literals(&mut self, ty: &Type, span: Span) {
        for var in self.checker.free_vars(ty) {
            if let Some(kind) = self.checker.unifier.literal_kind(var) {
                self.default_literal(var, kind, span);
            }
        }
    }

    fn finalize_wrapped_function(&mut self, expr: &mut TypedExpr) {
        if let TypedExprKind::Fn(lambda) = &mut expr.kind
            && let Some(wrapped) = &mut Rc::make_mut(lambda).wraps
        {
            self.finalize_type(&mut wrapped.ty, expr.span);
        }
    }

    fn note_closure(&mut self, expr: &TypedExpr) {
        if let TypedExprKind::Fn(_) = expr.kind {
            self.closures_in_definition.push(expr.ty.clone());
        }
    }

    fn finalize_type(&mut self, ty: &mut Type, span: Span) {
        for var in self.checker.free_vars(ty) {
            self.default_var(var, span, ty);
        }
        *ty = self.checker.unifier.resolve(ty);
    }

    fn default_var(&mut self, var: TypeVar, span: Span, ty: &Type) {
        match self.checker.unifier.literal_kind(var) {
            Some(kind) => self.default_literal(var, kind, span),
            None => self.report_unresolved(var, span, ty),
        }
    }

    fn default_literal(&mut self, var: TypeVar, kind: LiteralKind, span: Span) {
        let default = match kind {
            LiteralKind::Integer => PrimitiveType::I64,
            LiteralKind::Float => PrimitiveType::F64,
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

    fn report_unresolved(&mut self, var: TypeVar, span: Span, ty: &Type) {
        if !self.inside_operand && self.reported.insert(var) && !self.earlier_errors {
            let problem = self.missing_annotation(span, ty);
            self.checker.defer_missing_annotation(vec![var], problem);
        }
    }

    fn missing_annotation(&self, span: Span, ty: &Type) -> Diagnostic {
        match self.checker.evaluated_form(span) {
            Some(form) => {
                let open_type = self
                    .checker
                    .display()
                    .show(&self.checker.unifier.resolve(ty));
                TypeProblems::cannot_infer_evaluated(&open_type, form, span)
            }
            None => TypeProblems::cannot_infer(span),
        }
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
