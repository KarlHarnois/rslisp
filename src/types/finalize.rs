use std::collections::HashSet;
use std::rc::Rc;

use super::checker::TypeChecker;
use super::problems::TypeProblems;
use super::ty::{Type, TypeVar};
use super::typed::{TypedBlock, TypedBodyItem, TypedDefinition, TypedExpr, TypedExprKind};
use super::unifier::LiteralKind;
use crate::core::{Literal, PrimitiveType};
use crate::reader::{Number, NumberValue};
use crate::source::Span;

pub struct Finalizer<'c> {
    checker: &'c mut TypeChecker,
    reported: HashSet<TypeVar>,
    earlier_errors: bool,
}

impl<'c> Finalizer<'c> {
    pub fn new(checker: &'c mut TypeChecker) -> Self {
        let earlier_errors = checker.diagnostics.has_errors();
        Self {
            checker,
            reported: HashSet::new(),
            earlier_errors,
        }
    }

    pub fn definition(&mut self, definition: &mut TypedDefinition) {
        self.expr(&mut definition.value);
        self.finalize_type(&mut definition.scheme.body, definition.span);
    }

    fn expr(&mut self, expr: &mut TypedExpr) {
        self.finalize_type(&mut expr.ty, expr.span);
        match &mut expr.kind {
            TypedExprKind::Literal(Literal::Number(number)) => {
                self.check_range(number, &expr.ty.clone(), expr.span)
            }
            TypedExprKind::Fn(lambda) => {
                let lambda = Rc::make_mut(lambda);
                lambda
                    .params
                    .iter_mut()
                    .for_each(|param| self.finalize_type(&mut param.ty, param.span));
                self.block(&mut lambda.body);
            }
            TypedExprKind::Call(callee, arguments) => {
                self.expr(callee);
                arguments
                    .iter_mut()
                    .for_each(|argument| self.expr(&mut argument.value));
            }
            TypedExprKind::Block(block) => self.block(block),
            TypedExprKind::If(test, then, otherwise) => [test, then, otherwise]
                .into_iter()
                .for_each(|part| self.expr(part)),
            TypedExprKind::New(_, fields) => {
                fields.iter_mut().for_each(|(_, value)| self.expr(value))
            }
            TypedExprKind::Field(value, _) => self.expr(value),
            TypedExprKind::Literal(Literal::String(_))
            | TypedExprKind::Local(_)
            | TypedExprKind::Global(_)
            | TypedExprKind::Rust(_) => {}
        }
    }

    fn block(&mut self, block: &mut TypedBlock) {
        for item in &mut block.items {
            match item {
                TypedBodyItem::Let(binder, value) => {
                    self.expr(value);
                    self.finalize_type(&mut binder.ty, binder.span);
                }
                TypedBodyItem::Expr(expr) => self.expr(expr),
            }
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
                        .diagnostics
                        .push(TypeProblems::cannot_infer(span));
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
        let (NumberValue::Integer(value), Type::Primitive(primitive)) = (number.value(), ty) else {
            return;
        };
        let out_of_range = IntegerRange::of(*primitive)
            .is_some_and(|(minimum, maximum)| *value < minimum || *value > maximum);
        if out_of_range {
            let diagnostic = TypeProblems::out_of_range(number.text(), primitive.name(), span);
            self.checker.diagnostics.push(diagnostic);
        }
    }
}

struct IntegerRange;

impl IntegerRange {
    fn of(primitive: PrimitiveType) -> Option<(i128, i128)> {
        Some(match primitive {
            PrimitiveType::I8 => (i8::MIN.into(), i8::MAX.into()),
            PrimitiveType::I16 => (i16::MIN.into(), i16::MAX.into()),
            PrimitiveType::I32 => (i32::MIN.into(), i32::MAX.into()),
            PrimitiveType::I64 | PrimitiveType::Isize => (i64::MIN.into(), i64::MAX.into()),
            PrimitiveType::I128 => (i128::MIN, i128::MAX),
            PrimitiveType::U8 => (0, u8::MAX.into()),
            PrimitiveType::U16 => (0, u16::MAX.into()),
            PrimitiveType::U32 => (0, u32::MAX.into()),
            PrimitiveType::U64 | PrimitiveType::Usize => (0, u64::MAX.into()),
            PrimitiveType::U128 => (0, i128::MAX),
            _ => return None,
        })
    }
}
