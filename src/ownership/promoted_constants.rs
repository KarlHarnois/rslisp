use super::local_borrows::LocalBorrows;
use crate::core::{Division, Literal, Operand, ParsedTemplate, PrimitiveType, TemplateOperation};
use crate::reader::NumberValue;
use crate::types::{
    Type, TypedArgument, TypedControl, TypedExpr, TypedExprKind, TypedLeaf, TypedProgram,
};

#[derive(Clone, Copy)]
pub(super) struct PromotedConstants<'p> {
    program: &'p TypedProgram,
}

impl<'p> PromotedConstants<'p> {
    const ZERO: i128 = 0;
    const MINUS_ONE: i128 = -1;

    pub fn new(program: &'p TypedProgram) -> Self {
        Self { program }
    }

    pub fn is_temporary(&self, borrowed: &TypedExpr) -> bool {
        matches!(
            borrowed.kind,
            TypedExprKind::Call(..) | TypedExprKind::Control(_)
        ) && !self.promotes(borrowed)
    }

    fn promotes(&self, value: &TypedExpr) -> bool {
        match &value.kind {
            TypedExprKind::Call(callee, arguments) => self.promotes_operation(callee, arguments),
            TypedExprKind::Leaf(TypedLeaf::Rust(template)) => {
                ParsedTemplate::parse(&template.template).is_ok_and(|parsed| parsed.is_constant())
            }
            TypedExprKind::Control(TypedControl::Block(block)) => {
                LocalBorrows::tail_of(block).is_none_or(|tail| self.promotes(tail))
            }
            TypedExprKind::Leaf(TypedLeaf::Local(_)) | TypedExprKind::Control(_) => false,
            _ => value
                .children()
                .into_iter()
                .all(|child| self.promotes(child)),
        }
    }

    fn promotes_operation(&self, callee: &TypedExpr, arguments: &[TypedArgument]) -> bool {
        arguments
            .iter()
            .all(|argument| self.promotes(&argument.value))
            && self
                .program
                .template_borrows
                .read_template(callee, false, |template| {
                    self.promotes_template(template, arguments)
                })
    }

    fn promotes_template(&self, template: &ParsedTemplate, arguments: &[TypedArgument]) -> bool {
        let operation = template.operation();
        operation.is_tuple()
            || operation.is_operator()
                && self.operates_on_builtins(arguments)
                && self.divides_by_constants(&operation, arguments)
    }

    fn operates_on_builtins(&self, arguments: &[TypedArgument]) -> bool {
        arguments
            .iter()
            .all(|argument| self.is_builtin_operand(&argument.value.ty))
    }

    fn is_builtin_operand(&self, ty: &Type) -> bool {
        self.primitive(ty).is_some_and(|primitive| {
            primitive.is_integer()
                || primitive.is_float()
                || matches!(primitive, PrimitiveType::Bool | PrimitiveType::Char)
        })
    }

    fn primitive(&self, ty: &Type) -> Option<PrimitiveType> {
        match ty {
            Type::Primitive(primitive) => Some(*primitive),
            Type::Generic(generic) => self
                .program
                .literal_generics
                .get(generic)
                .map(|literal| literal.default_type()),
            _ => None,
        }
    }

    fn divides_by_constants(
        &self,
        operation: &TemplateOperation,
        arguments: &[TypedArgument],
    ) -> bool {
        operation
            .divisions()
            .iter()
            .all(|division| self.promotes_division(division, arguments))
    }

    fn promotes_division(&self, division: &Division, arguments: &[TypedArgument]) -> bool {
        match division.divisor {
            Operand::Argument(position) => arguments.get(position).is_some_and(|divisor| {
                self.promotes_argument_division(division.dividend, &divisor.value, arguments)
            }),
            Operand::Constant(value) => {
                self.promotes_constant_division(division.dividend, value, arguments)
            }
        }
    }

    fn promotes_argument_division(
        &self,
        dividend: Option<Operand>,
        divisor: &TypedExpr,
        arguments: &[TypedArgument],
    ) -> bool {
        !self.is_integer(&divisor.ty)
            || self.promotes_integer_division(dividend, divisor, arguments)
    }

    fn promotes_constant_division(
        &self,
        dividend: Option<Operand>,
        divisor: Option<i128>,
        arguments: &[TypedArgument],
    ) -> bool {
        match divisor {
            Some(Self::ZERO) => false,
            Some(Self::MINUS_ONE) => self.is_constant_dividend(dividend, arguments),
            _ => true,
        }
    }

    fn is_integer(&self, ty: &Type) -> bool {
        self.primitive(ty)
            .is_some_and(|primitive| primitive.is_integer())
    }

    fn promotes_integer_division(
        &self,
        dividend: Option<Operand>,
        divisor: &TypedExpr,
        arguments: &[TypedArgument],
    ) -> bool {
        let cannot_divide_by_zero =
            self.is_constant_operand(divisor) && !self.has_value(divisor, Self::ZERO);
        let cannot_overflow = !self.has_value(divisor, Self::MINUS_ONE)
            || self.is_constant_dividend(dividend, arguments);
        cannot_divide_by_zero && cannot_overflow
    }

    fn is_constant_dividend(&self, dividend: Option<Operand>, arguments: &[TypedArgument]) -> bool {
        match dividend {
            Some(Operand::Argument(position)) => arguments
                .get(position)
                .is_some_and(|argument| self.is_constant_operand(&argument.value)),
            Some(Operand::Constant(_)) | None => true,
        }
    }

    fn is_constant_operand(&self, operand: &TypedExpr) -> bool {
        match &operand.kind {
            TypedExprKind::Leaf(TypedLeaf::Literal(_) | TypedLeaf::Global(_)) => true,
            _ => self.negated(operand).is_some_and(Self::is_literal),
        }
    }

    fn has_value(&self, operand: &TypedExpr, value: i128) -> bool {
        Self::is_literal_of(operand, value)
            || self
                .negated(operand)
                .is_some_and(|negated| Self::is_literal_of(negated, -value))
    }

    fn negated<'e>(&self, operand: &'e TypedExpr) -> Option<&'e TypedExpr> {
        let TypedExprKind::Call(callee, arguments) = &operand.kind else {
            return None;
        };
        let negates = self
            .program
            .template_borrows
            .read_template(callee, false, |template| template.operation().is_negation());
        arguments
            .first()
            .filter(|_| negates)
            .map(|argument| &argument.value)
    }

    fn is_literal(operand: &TypedExpr) -> bool {
        matches!(operand.kind, TypedExprKind::Leaf(TypedLeaf::Literal(_)))
    }

    fn is_literal_of(operand: &TypedExpr, value: i128) -> bool {
        matches!(&operand.kind, TypedExprKind::Leaf(TypedLeaf::Literal(Literal::Number(number)))
            if *number.value() == NumberValue::Integer(value))
    }
}
