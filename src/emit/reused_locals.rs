use rustc_hash::FxHashMap;

use super::expressions::{ExpressionEmitter, TailLoop};
use crate::core::BindingId;
use crate::types::{
    Borrow, TypedArgument, TypedBinder, TypedBlock, TypedBodyItem, TypedControl, TypedExpr,
    TypedExprKind, TypedLambda, TypedLeaf, TypedProgram,
};

pub struct ReusedLocals {
    reuses: FxHashMap<BindingId, Reuse>,
}

#[derive(Clone, Copy)]
struct Reuse {
    local: BindingId,
    reassigned: bool,
}

struct ReuseSearch<'a> {
    program: &'a TypedProgram,
    reuses: FxHashMap<BindingId, Reuse>,
}

struct InlineCall<'a> {
    body: &'a TypedBlock,
    position: usize,
    lambda: &'a TypedLambda,
    arguments: &'a [TypedArgument],
}

impl ReusedLocals {
    pub fn of(program: &TypedProgram, value: &TypedExpr) -> Self {
        let mut search = ReuseSearch {
            program,
            reuses: FxHashMap::default(),
        };
        search.expr(value);
        Self {
            reuses: search.reuses,
        }
    }

    pub fn empty() -> Self {
        Self {
            reuses: FxHashMap::default(),
        }
    }

    pub fn local_of(&self, parameter: BindingId) -> Option<BindingId> {
        self.reuses.get(&parameter).map(|reuse| reuse.local)
    }

    pub fn changed_locals(&self, changes_in_place: impl Fn(BindingId) -> bool) -> Vec<BindingId> {
        self.reuses
            .iter()
            .filter(|(parameter, reuse)| reuse.reassigned || changes_in_place(**parameter))
            .map(|(_, reuse)| reuse.local)
            .collect()
    }
}

impl ReuseSearch<'_> {
    fn expr(&mut self, expr: &TypedExpr) {
        match &expr.kind {
            TypedExprKind::Fn(lambda) => self.block(&lambda.body),
            TypedExprKind::Control(TypedControl::Block(block)) => self.block(block),
            _ => self.children(expr),
        }
    }

    fn children(&mut self, expr: &TypedExpr) {
        for child in expr.children() {
            self.expr(child);
        }
    }

    fn block(&mut self, body: &TypedBlock) {
        for (position, item) in body.items.iter().enumerate() {
            if let Some(call) = InlineCall::at(body, position) {
                self.reuse_locals(&call);
            }
            self.expr(item.expr());
        }
    }

    fn reuse_locals(&mut self, call: &InlineCall) {
        let tail_loop = TailLoop::of_closure(call.lambda);
        for (index, parameter) in call.lambda.params.iter().enumerate() {
            if let Some(local) = self.reusable_local(call, index, parameter) {
                let reassigned = tail_loop
                    .as_ref()
                    .is_some_and(|tail_loop| tail_loop.reassigns(parameter.binding));
                self.reuses
                    .insert(parameter.binding, Reuse { local, reassigned });
            }
        }
    }

    fn reusable_local(
        &self,
        call: &InlineCall,
        index: usize,
        parameter: &TypedBinder,
    ) -> Option<BindingId> {
        let local = call.passed_local(index)?;
        call.earlier_binder(local)?;
        let same_name = self.name(local) == self.name(parameter.binding);
        (same_name && !call.reads_from_the_call_on(local, index)).then_some(local)
    }

    fn name(&self, binding: BindingId) -> &str {
        self.program.bindings.get(binding).name()
    }
}

impl<'a> InlineCall<'a> {
    fn at(body: &'a TypedBlock, position: usize) -> Option<Self> {
        let TypedBodyItem::Expr(expr) = &body.items[position] else {
            return None;
        };
        let TypedExprKind::Call(callee, arguments) = &expr.kind else {
            return None;
        };
        let TypedExprKind::Fn(lambda) = &callee.kind else {
            return None;
        };
        (!ExpressionEmitter::contains_try(&lambda.body)).then_some(Self {
            body,
            position,
            lambda,
            arguments,
        })
    }

    fn passed_local(&self, index: usize) -> Option<BindingId> {
        let argument = self.arguments.get(index)?;
        match (&argument.value.kind, argument.borrow) {
            (TypedExprKind::Leaf(TypedLeaf::Local(local)), Borrow::None) => Some(*local),
            _ => None,
        }
    }

    fn earlier_binder(&self, local: BindingId) -> Option<&'a TypedBinder> {
        self.earlier_bindings()
            .map(|(binder, _)| binder)
            .find(|binder| binder.binding == local)
    }

    fn earlier_bindings(&self) -> impl Iterator<Item = (&'a TypedBinder, &'a TypedExpr)> {
        self.body.items[..self.position]
            .iter()
            .filter_map(|item| match item {
                TypedBodyItem::Let(binder, value) => Some((binder, value)),
                TypedBodyItem::Expr(_) => None,
            })
    }

    fn reads_from_the_call_on(&self, local: BindingId, index: usize) -> bool {
        self.reads_after_its_argument(local, index) || self.live_borrower(local)
    }

    fn reads_after_its_argument(&self, local: BindingId, index: usize) -> bool {
        let other_arguments = self
            .arguments
            .iter()
            .enumerate()
            .filter(|(position, _)| *position != index)
            .any(|(_, argument)| argument.value.mentions_local(local));
        other_arguments || self.reads_in_the_call_or_after(local)
    }

    fn reads_in_the_call_or_after(&self, local: BindingId) -> bool {
        let in_body = self
            .lambda
            .body
            .expressions()
            .any(|expr| expr.mentions_local(local));
        let after = self.body.items[self.position + 1..]
            .iter()
            .any(|item| item.expr().mentions_local(local));
        in_body || after
    }

    fn live_borrower(&self, local: BindingId) -> bool {
        self.earlier_bindings()
            .filter(|(binder, value)| Self::can_borrow(binder) && value.mentions_local(local))
            .any(|(binder, _)| {
                let passed = self
                    .arguments
                    .iter()
                    .any(|argument| argument.value.mentions_local(binder.binding));
                passed || self.reads_in_the_call_or_after(binder.binding)
            })
    }

    fn can_borrow(binder: &TypedBinder) -> bool {
        binder.ty.holds_reference() || binder.ty.holds_function()
    }
}
