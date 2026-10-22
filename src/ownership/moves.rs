use std::collections::{HashMap, HashSet};

use super::definitions::LocalDefinitions;
use super::problems::OwnershipProblems;
use crate::core::{BindingArena, BindingId};
use crate::diagnostic::Diagnostic;
use crate::source::Span;
use crate::types::{
    Borrow, CopyRules, Type, TypedArgument, TypedBlock, TypedBodyItem, TypedControl, TypedExpr,
    TypedExprKind, TypedLambda, TypedLeaf, TypedStructExpr,
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
    copy_functions: HashSet<BindingId>,
    reported: HashSet<BindingId>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> MoveChecker<'a> {
    pub fn new(copy: CopyRules<'a>, bindings: &'a BindingArena) -> Self {
        Self {
            copy,
            bindings,
            moved: HashMap::new(),
            copy_functions: HashSet::new(),
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
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => {
                self.local(*binding, &expr.ty, expr.span, usage)
            }
            TypedExprKind::Leaf(_) => {}
            TypedExprKind::Call(callee, arguments) => self.call(callee, arguments),
            TypedExprKind::Struct(structure) => self.structure(structure),
            TypedExprKind::Control(control) => self.control(control, usage),
            TypedExprKind::Fn(lambda) => {
                self.closure(lambda);
            }
        }
    }

    fn structure(&mut self, structure: &TypedStructExpr) {
        match structure {
            TypedStructExpr::Field(value, _) => self.expr(value, Usage::Borrow),
            TypedStructExpr::New(_, fields) => fields
                .iter()
                .for_each(|(_, value)| self.expr(value, Usage::Move)),
        }
    }

    fn control(&mut self, control: &TypedControl, usage: Usage) {
        match control {
            TypedControl::Block(block) => self.block(block, usage),
            TypedControl::If(test, then, otherwise) => self.branches(test, then, otherwise, usage),
        }
    }

    fn closure(&mut self, lambda: &TypedLambda) -> bool {
        let own_bindings = LocalDefinitions::of(lambda);
        let moved_before: HashSet<BindingId> = self.moved.keys().copied().collect();
        self.block(&lambda.body, Usage::Move);
        let moves_a_capture = self
            .moved
            .keys()
            .any(|binding| !moved_before.contains(binding) && !own_bindings.contains(binding));
        !moves_a_capture
    }

    fn bind(&mut self, binding: BindingId, value: &TypedExpr) {
        let is_copy_function = match &value.kind {
            TypedExprKind::Fn(lambda) => self.closure(lambda),
            TypedExprKind::Leaf(TypedLeaf::Global(_)) | TypedExprKind::Leaf(TypedLeaf::Rust(_)) => {
                matches!(value.ty, Type::Function(..))
            }
            TypedExprKind::Leaf(TypedLeaf::Local(other)) => {
                let is_copy = self.copy_functions.contains(other);
                self.expr(value, Usage::Move);
                is_copy
            }
            _ => {
                self.expr(value, Usage::Move);
                false
            }
        };
        if is_copy_function {
            self.copy_functions.insert(binding);
        }
    }

    fn call(&mut self, callee: &TypedExpr, arguments: &[TypedArgument]) {
        if !matches!(
            callee.kind,
            TypedExprKind::Leaf(TypedLeaf::Global(_)) | TypedExprKind::Leaf(TypedLeaf::Rust(_))
        ) {
            self.expr(callee, Usage::Borrow);
        }
        for argument in arguments {
            let moves =
                argument.borrow == Borrow::None && !matches!(argument.value.ty, Type::Ref(_));
            let usage = if moves { Usage::Move } else { Usage::Borrow };
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
                TypedBodyItem::Let(binder, value) => self.bind(binder.binding, value),
                TypedBodyItem::Expr(expr) if Some(position) == last => self.expr(expr, usage),
                TypedBodyItem::Expr(expr) => self.expr(expr, Usage::Move),
            }
        }
    }

    fn local(&mut self, binding: BindingId, ty: &Type, span: Span, usage: Usage) {
        if let Some(moved_at) = self.moved.get(&binding).copied() {
            if self.reported.insert(binding) {
                let name = self.bindings.get(binding).name();
                let diagnostic = OwnershipProblems::use_after_move(name, span, moved_at);
                let diagnostic = if matches!(ty, Type::Function(..)) {
                    OwnershipProblems::closure_move_help(diagnostic)
                } else {
                    OwnershipProblems::clone_suggestion(diagnostic, name, moved_at)
                };
                self.diagnostics.push(diagnostic);
            }
            return;
        }
        let is_copy = self.copy.is_copy(ty) || self.copy_functions.contains(&binding);
        if usage == Usage::Move && !is_copy {
            self.moved.insert(binding, span);
        }
    }
}
