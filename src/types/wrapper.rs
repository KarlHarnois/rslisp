use std::rc::Rc;

use super::ty::Type;
use super::typed::{
    Borrow, TypedArgument, TypedBinder, TypedBlock, TypedBodyItem, TypedExpr, TypedExprKind,
    TypedLambda, TypedLeaf,
};
use crate::core::{BindingArena, BindingId, BindingInfo, BindingKind};
use crate::source::Span;

pub struct FunctionValueWrapper<'a> {
    bindings: &'a mut BindingArena,
}

impl<'a> FunctionValueWrapper<'a> {
    pub fn new(bindings: &'a mut BindingArena) -> Self {
        Self { bindings }
    }

    pub fn prepare(
        &mut self,
        expected_parameters: &[Type],
        value: TypedExpr,
        wraps: Option<BindingId>,
    ) -> PreparedFunctionValue {
        let span = value.span;
        let mut params = Vec::new();
        let mut arguments = Vec::new();
        for expected in expected_parameters {
            let binding = self.bindings.add(BindingInfo::new(
                "argument".into(),
                BindingKind::Local,
                None,
            ));
            let local = TypedExpr {
                kind: TypedExprKind::Leaf(TypedLeaf::Local(binding)),
                ty: expected.clone(),
                span,
            };
            params.push(TypedBinder {
                binding,
                ty: expected.clone(),
                span,
                annotated: false,
            });
            arguments.push(TypedArgument {
                value: local,
                borrow: Borrow::None,
            });
        }
        PreparedFunctionValue {
            params,
            arguments,
            value,
            span,
            wraps,
        }
    }
}

pub struct PreparedFunctionValue {
    pub params: Vec<TypedBinder>,
    pub arguments: Vec<TypedArgument>,
    value: TypedExpr,
    span: Span,
    wraps: Option<BindingId>,
}

impl PreparedFunctionValue {
    pub fn span(&self) -> Span {
        self.span
    }

    pub fn finish(self, expected_result: &Type, found_result: &Type) -> TypedExpr {
        let parameters: Vec<Type> = self.params.iter().map(|param| param.ty.clone()).collect();
        let call = TypedExpr {
            kind: TypedExprKind::Call(Box::new(self.value), self.arguments),
            ty: found_result.clone(),
            span: self.span,
        };
        let lambda = TypedLambda {
            params: self.params,
            body: TypedBlock {
                items: vec![TypedBodyItem::Expr(call)],
            },
            wraps: self.wraps,
        };
        TypedExpr {
            kind: TypedExprKind::Fn(Rc::new(lambda)),
            ty: Type::Function(parameters, Box::new(expected_result.clone())),
            span: self.span,
        }
    }
}
