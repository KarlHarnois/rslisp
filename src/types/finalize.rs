use std::rc::Rc;

use rustc_hash::FxHashSet;

use super::checker::{Admissible, TypeChecker};
use super::literal_fit::LiteralFit;
use super::problems::TypeProblems;
use super::ty::{GenericId, Type, TypeVar};
use super::type_text::TypeTextWriter;
use super::typed::{ClosureType, LetValue, TypedDefinition, TypedExpr, TypedExprKind, TypedLeaf};
use super::unifier::LiteralKind;
use crate::core::{BindingId, Literal, PrimitiveType};
use crate::diagnostic::Diagnostic;
use crate::reader::Number;
use crate::source::Span;

#[derive(Clone, Copy)]
enum OpenSite {
    WrittenValue(Span),
    Elsewhere(Span),
}

impl OpenSite {
    fn span(self) -> Span {
        match self {
            Self::WrittenValue(span) | Self::Elsewhere(span) => span,
        }
    }
}

#[derive(Clone, Copy)]
enum Enclosure {
    Form(Span),
    Binder(Span),
    AnnotatedBinder(Span),
}

impl Enclosure {
    fn of_child(parent_span: Span, let_value: Option<LetValue>) -> Self {
        match let_value {
            Some(value) if value.annotated => Self::AnnotatedBinder(value.binder_span),
            Some(value) => Self::Binder(value.binder_span),
            None => Self::Form(parent_span),
        }
    }

    fn site_of(enclosure: Option<Self>, span: Span) -> OpenSite {
        match enclosure {
            Some(Self::AnnotatedBinder(binder_span)) => OpenSite::Elsewhere(binder_span),
            Some(Self::Form(form_span)) if form_span == span => OpenSite::Elsewhere(span),
            Some(Self::Binder(binder_span)) if span.contains(binder_span) => {
                OpenSite::Elsewhere(span)
            }
            _ => OpenSite::WrittenValue(span),
        }
    }
}

pub struct Finalizer<'c> {
    checker: &'c mut TypeChecker,
    reported: FxHashSet<TypeVar>,
    earlier_errors: bool,
    closures_in_definition: Vec<Type>,
    type_text_operands: Vec<TypedExpr>,
    inside_operand: bool,
    enclosure: Option<Enclosure>,
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
            enclosure: None,
        }
    }

    pub fn definition(&mut self, definition: &mut TypedDefinition) {
        self.enclosure = Some(Enclosure::Form(definition.span));
        self.expr(&mut definition.value);
        self.enclosure = None;
        self.finalize_type(
            &mut definition.scheme.body,
            OpenSite::Elsewhere(definition.span),
        );
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
        let enclosure = self.enclosure;
        let site = Enclosure::site_of(enclosure, expr.span);
        if let TypedExprKind::Call(callee, arguments) = &mut expr.kind {
            if self.checker.unifier.holds_open_literal(&callee.ty) {
                self.default_literals(&callee.ty, callee.span);
            }
            self.enclosure = Some(Enclosure::Form(expr.span));
            for argument in arguments {
                self.child(&mut argument.value);
            }
            self.finalize_type(&mut expr.ty, site);
            self.child(callee);
        } else {
            self.children(expr);
            self.finalize_type(&mut expr.ty, site);
        }
        self.enclosure = enclosure;
        if let TypedExprKind::Leaf(TypedLeaf::Literal(Literal::Number(number))) = &expr.kind {
            self.check_range(number, &expr.ty.clone(), expr.span);
        }
        self.finalize_wrapped_function(expr);
    }

    fn children(&mut self, expr: &mut TypedExpr) {
        let parent_span = expr.span;
        expr.visit_children_mut(&mut |let_value, child| {
            self.enclosure = Some(Enclosure::of_child(parent_span, let_value));
            self.child(child);
        });
        for binder in expr.binders_mut() {
            self.finalize_type(&mut binder.ty, OpenSite::Elsewhere(binder.span));
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
            self.finalize_type(&mut wrapped.ty, OpenSite::Elsewhere(expr.span));
        }
    }

    fn note_closure(&mut self, expr: &TypedExpr) {
        if let TypedExprKind::Fn(_) = expr.kind {
            self.closures_in_definition.push(expr.ty.clone());
        }
    }

    fn finalize_type(&mut self, ty: &mut Type, site: OpenSite) {
        for var in self.checker.free_vars(ty) {
            self.default_var(var, site, ty);
        }
        *ty = self.checker.unifier.resolve(ty);
    }

    fn default_var(&mut self, var: TypeVar, site: OpenSite, ty: &Type) {
        match self.checker.unifier.literal_kind(var) {
            Some(kind) => self.default_literal(var, kind, site.span()),
            None => self.report_unresolved(var, site, ty),
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

    fn report_unresolved(&mut self, var: TypeVar, site: OpenSite, ty: &Type) {
        if !self.inside_operand && self.reported.insert(var) && !self.earlier_errors {
            let problem = self.missing_annotation(site, ty);
            self.checker.defer_missing_annotation(vec![var], problem);
        }
    }

    fn missing_annotation(&self, site: OpenSite, ty: &Type) -> Diagnostic {
        match site {
            OpenSite::WrittenValue(span) => {
                let open_type = self
                    .checker
                    .display()
                    .show(&self.checker.unifier.resolve(ty));
                TypeProblems::cannot_infer_value(&open_type, span)
            }
            OpenSite::Elsewhere(span) => TypeProblems::cannot_infer(span),
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
