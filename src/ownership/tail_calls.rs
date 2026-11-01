use rustc_hash::FxHashSet;

use crate::core::BindingId;
use crate::types::{
    Borrow, TypedArgument, TypedBlock, TypedBodyItem, TypedControl, TypedExpr, TypedExprKind,
    TypedLambda, TypedLeaf, TypedStructExpr,
};

pub struct TailCalls<'e> {
    function: Option<BindingId>,
    calls: Vec<&'e [TypedArgument]>,
    loop_locals: FxHashSet<BindingId>,
}

impl<'e> TailCalls<'e> {
    pub fn of(function: Option<BindingId>, lambda: &'e TypedLambda) -> Self {
        let mut tail_calls = Self {
            function,
            calls: Vec::new(),
            loop_locals: FxHashSet::default(),
        };
        tail_calls.collect_block(&lambda.body);
        tail_calls
    }

    pub fn arguments(&self) -> &[&'e [TypedArgument]] {
        &self.calls
    }

    pub fn recurrence(function: Option<BindingId>, expr: &TypedExpr) -> Option<&[TypedArgument]> {
        match &expr.kind {
            TypedExprKind::Recur(arguments) => Some(arguments),
            TypedExprKind::Call(callee, arguments) if Self::calls_itself(function, callee) => {
                Some(arguments)
            }
            _ => None,
        }
    }

    fn calls_itself(function: Option<BindingId>, callee: &TypedExpr) -> bool {
        let Some(function) = function else {
            return false;
        };
        matches!(callee.kind, TypedExprKind::Leaf(TypedLeaf::Global(binding)) if binding == function)
    }

    pub fn rebuilt_parameters(&self, lambda: &TypedLambda) -> FxHashSet<BindingId> {
        self.calls
            .iter()
            .flat_map(|arguments| {
                lambda
                    .params
                    .iter()
                    .zip(arguments.iter())
                    .filter(|(_, argument)| self.is_new_value(argument))
                    .map(|(param, _)| param.binding)
            })
            .collect()
    }

    fn is_new_value(&self, argument: &TypedArgument) -> bool {
        argument.borrow == Borrow::None
            && !argument.value.ty.is_reference()
            && !self.outlives_loop(&argument.value)
    }

    fn outlives_loop(&self, expr: &TypedExpr) -> bool {
        match &expr.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => !self.loop_locals.contains(binding),
            TypedExprKind::Struct(TypedStructExpr::Field(owner, _)) => self.outlives_loop(owner),
            _ => false,
        }
    }

    fn collect_block(&mut self, block: &'e TypedBlock) {
        self.loop_locals
            .extend(block.items.iter().filter_map(Self::let_binding));
        if let Some(TypedBodyItem::Expr(tail)) = block.items.last() {
            self.collect(tail);
        }
    }

    fn let_binding(item: &TypedBodyItem) -> Option<BindingId> {
        match item {
            TypedBodyItem::Let(binder, _) => Some(binder.binding),
            TypedBodyItem::Expr(_) => None,
        }
    }

    fn collect(&mut self, expr: &'e TypedExpr) {
        if let Some(arguments) = Self::recurrence(self.function, expr) {
            self.calls.push(arguments)
        }
        if let TypedExprKind::Control(control) = &expr.kind {
            self.collect_control(control)
        }
    }

    fn collect_control(&mut self, control: &'e TypedControl) {
        match control {
            TypedControl::If(_, then, otherwise) => {
                self.collect(then);
                self.collect(otherwise);
            }
            TypedControl::Block(block) => self.collect_block(block),
            TypedControl::Match(matched) => matched
                .arms
                .iter()
                .for_each(|arm| self.collect_block(&arm.body)),
        }
    }
}
