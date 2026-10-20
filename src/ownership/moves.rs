use std::collections::{HashMap, HashSet};

use super::problems::OwnershipProblems;
use crate::core::{BindingArena, BindingId};
use crate::diagnostic::Diagnostic;
use crate::source::Span;
use crate::types::{
    Borrow, CopyRules, Type, TypedArgument, TypedBlock, TypedBodyItem, TypedExpr, TypedExprKind,
    TypedLambda,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Usage {
    Move,
    Borrow,
}

pub struct MoveChecker<'a> {
    copy: CopyRules<'a>,
    bindings: &'a BindingArena,
    moved: HashMap<BindingId, Span>,
    reported: HashSet<BindingId>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> MoveChecker<'a> {
    pub fn new(copy: CopyRules<'a>, bindings: &'a BindingArena) -> Self {
        Self {
            copy,
            bindings,
            moved: HashMap::new(),
            reported: HashSet::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn check(mut self, lambda: &TypedLambda) -> Vec<Diagnostic> {
        self.block(&lambda.body, Usage::Move);
        self.diagnostics
    }

    fn expr(&mut self, expr: &TypedExpr, usage: Usage) {
        match &expr.kind {
            TypedExprKind::Local(binding) => self.local(*binding, &expr.ty, expr.span, usage),
            TypedExprKind::Call(callee, arguments) => self.call(callee, arguments),
            TypedExprKind::Field(value, _) => self.expr(value, Usage::Borrow),
            TypedExprKind::New(_, fields) => fields
                .iter()
                .for_each(|(_, value)| self.expr(value, Usage::Move)),
            TypedExprKind::Block(block) => self.block(block, usage),
            TypedExprKind::If(test, then, otherwise) => self.branches(test, then, otherwise, usage),
            TypedExprKind::Fn(lambda) => self.block(&lambda.body, Usage::Move),
            TypedExprKind::Literal(_) | TypedExprKind::Global(_) | TypedExprKind::Rust(_) => {}
        }
    }

    fn call(&mut self, callee: &TypedExpr, arguments: &[TypedArgument]) {
        if !matches!(
            callee.kind,
            TypedExprKind::Global(_) | TypedExprKind::Rust(_)
        ) {
            self.expr(callee, Usage::Borrow);
        }
        for argument in arguments {
            let usage = if argument.borrow == Borrow::None {
                Usage::Move
            } else {
                Usage::Borrow
            };
            self.expr(&argument.value, usage);
        }
    }

    fn branches(
        &mut self,
        test: &TypedExpr,
        then: &TypedExpr,
        otherwise: &TypedExpr,
        usage: Usage,
    ) {
        self.expr(test, Usage::Borrow);
        let before = self.moved.clone();
        self.expr(then, usage);
        let after_then = std::mem::replace(&mut self.moved, before);
        self.expr(otherwise, usage);
        self.moved.extend(after_then);
    }

    fn block(&mut self, block: &TypedBlock, usage: Usage) {
        let last = block.items.len().checked_sub(1);
        for (position, item) in block.items.iter().enumerate() {
            match item {
                TypedBodyItem::Let(_, value) => self.expr(value, Usage::Move),
                TypedBodyItem::Expr(expr) if Some(position) == last => self.expr(expr, usage),
                TypedBodyItem::Expr(expr) => self.expr(expr, Usage::Move),
            }
        }
    }

    fn local(&mut self, binding: BindingId, ty: &Type, span: Span, usage: Usage) {
        if let Some(moved_at) = self.moved.get(&binding).copied() {
            if self.reported.insert(binding) {
                let name = self.bindings.get(binding).name();
                self.diagnostics
                    .push(OwnershipProblems::use_after_move(name, span, moved_at));
            }
            return;
        }
        if usage == Usage::Move && !self.copy.is_copy(ty) {
            self.moved.insert(binding, span);
        }
    }
}
