use rustc_hash::FxHashSet;

use super::problems::DroppedValue;
use crate::core::BindingId;
use crate::types::{
    Borrow, Type, TypedArgument, TypedBlock, TypedBodyItem, TypedControl, TypedExpr, TypedExprKind,
    TypedLambda, TypedLeaf, TypedMatch, TypedProgram,
};

#[derive(Clone)]
pub(super) enum ResultScope {
    Discarded,
    Statement,
    Outliving(Vec<BindingId>),
}

impl ResultScope {
    pub(super) fn outlives(&self, dropped: &DroppedValue) -> bool {
        match self {
            Self::Discarded | Self::Statement => false,
            Self::Outliving(dropped_first) => {
                dropped.is_temporary()
                    || dropped
                        .binding()
                        .is_some_and(|binding| dropped_first.contains(&binding))
            }
        }
    }

    fn kept(&self) -> Self {
        match self {
            Self::Discarded => Self::Statement,
            kept => kept.clone(),
        }
    }

    fn outliving_with(&self, locals: &[BindingId]) -> Self {
        let mut dropped_first = match self {
            Self::Outliving(dropped_first) => dropped_first.clone(),
            Self::Discarded | Self::Statement => Vec::new(),
        };
        dropped_first.extend_from_slice(locals);
        Self::Outliving(dropped_first)
    }

    fn of_block_tail(&self, locals: &[BindingId]) -> Self {
        match self {
            Self::Discarded => Self::Discarded,
            outer => outer.outliving_with(locals),
        }
    }

    fn of_block_let(&self, locals: &[BindingId]) -> Self {
        match self {
            Self::Discarded => Self::Outliving(Vec::new()),
            outer => outer.outliving_with(locals),
        }
    }
}

pub(super) struct ScopedCall<'e> {
    pub called: BindingId,
    pub arguments: &'e [TypedArgument],
    pub scope: ResultScope,
}

struct ScopedBinding {
    binding: BindingId,
    depth: usize,
    parameter: bool,
}

impl ScopedBinding {
    const OUTSIDE_DEFINITION: usize = 0;

    fn dropped_before(&self, place_depth: usize) -> bool {
        self.depth > place_depth || (self.parameter && self.depth == place_depth)
    }
}

pub(super) struct CallScopes<'e> {
    program: &'e TypedProgram,
    local_functions: &'e FxHashSet<BindingId>,
    in_scope: Vec<ScopedBinding>,
    depth: usize,
    calls: Vec<ScopedCall<'e>>,
}

impl<'e> CallScopes<'e> {
    pub(super) fn of(
        program: &'e TypedProgram,
        local_functions: &'e FxHashSet<BindingId>,
        value: &'e TypedExpr,
    ) -> Vec<ScopedCall<'e>> {
        let mut scopes = Self {
            program,
            local_functions,
            in_scope: Vec::new(),
            depth: 0,
            calls: Vec::new(),
        };
        scopes.expr(value, &ResultScope::Discarded);
        scopes.calls
    }

    fn expr(&mut self, expr: &'e TypedExpr, scope: &ResultScope) {
        match &expr.kind {
            TypedExprKind::Fn(lambda) => self.lambda(lambda),
            TypedExprKind::Control(control) => {
                let valued = self.valued(&expr.ty, scope);
                self.control(control, &valued);
            }
            TypedExprKind::Call(callee, arguments) => self.call(expr, callee, arguments, scope),
            _ => self.parts(expr, scope),
        }
    }

    fn valued(&self, value: &Type, scope: &ResultScope) -> ResultScope {
        if self.program.type_holds_borrow(value) {
            scope.clone()
        } else {
            ResultScope::Discarded
        }
    }

    fn parts(&mut self, expr: &'e TypedExpr, scope: &ResultScope) {
        let parts = self.held_by(&expr.ty, scope);
        for child in expr.children() {
            self.expr(child, &parts);
        }
    }

    fn held_by(&self, holder: &Type, scope: &ResultScope) -> ResultScope {
        if self.program.type_holds_borrow(holder) {
            scope.kept()
        } else {
            ResultScope::Statement
        }
    }

    fn lambda(&mut self, lambda: &'e TypedLambda) {
        let outer = self.in_scope.len();
        self.depth += 1;
        let depth = self.depth;
        self.in_scope
            .extend(lambda.params.iter().map(|param| ScopedBinding {
                binding: param.binding,
                depth,
                parameter: true,
            }));
        self.block(&lambda.body, &ResultScope::Discarded);
        self.in_scope.truncate(outer);
        self.depth -= 1;
    }

    fn control(&mut self, control: &'e TypedControl, scope: &ResultScope) {
        match control {
            TypedControl::Block(block) => self.block(block, scope),
            TypedControl::If(condition, then, otherwise) => {
                self.expr(condition, &ResultScope::Statement);
                self.expr(then, scope);
                self.expr(otherwise, scope);
            }
            TypedControl::Match(matched) => self.matched(matched, scope),
        }
    }

    fn matched(&mut self, matched: &'e TypedMatch, scope: &ResultScope) {
        let scrutinee = self.held_by(&matched.value.ty, scope);
        self.expr(&matched.value, &scrutinee);
        for arm in &matched.arms {
            self.block(&arm.body, scope);
        }
    }

    fn block(&mut self, block: &'e TypedBlock, scope: &ResultScope) {
        let outer = self.in_scope.len();
        self.depth += 1;
        let last = block.items.len().saturating_sub(1);
        for (position, item) in block.items.iter().enumerate() {
            let block_locals = self.bindings_since(outer);
            match item {
                TypedBodyItem::Let(binder, value) => {
                    self.expr(value, &scope.of_block_let(&block_locals));
                    self.bind(binder.binding);
                }
                TypedBodyItem::Expr(tail) if position == last => {
                    self.expr(tail, &scope.of_block_tail(&block_locals));
                }
                TypedBodyItem::Expr(statement) => self.expr(statement, &ResultScope::Discarded),
            }
        }
        self.in_scope.truncate(outer);
        self.depth -= 1;
    }

    fn bindings_since(&self, outer: usize) -> Vec<BindingId> {
        self.in_scope[outer..]
            .iter()
            .map(|scoped| scoped.binding)
            .collect()
    }

    fn bind(&mut self, binding: BindingId) {
        self.in_scope.push(ScopedBinding {
            binding,
            depth: self.depth,
            parameter: false,
        });
    }

    fn call(
        &mut self,
        call: &'e TypedExpr,
        callee: &'e TypedExpr,
        arguments: &'e [TypedArgument],
        scope: &ResultScope,
    ) {
        self.record(callee, arguments, scope);
        let argument_scope = self.argument_scope(call, callee, arguments, scope);
        self.expr(callee, &ResultScope::Statement);
        for argument in arguments {
            self.expr(&argument.value, &argument_scope);
        }
    }

    fn argument_scope(
        &self,
        call: &TypedExpr,
        callee: &TypedExpr,
        arguments: &[TypedArgument],
        scope: &ResultScope,
    ) -> ResultScope {
        let stored: Vec<&TypedExpr> = arguments
            .iter()
            .filter(|argument| Self::passes_mut(argument))
            .map(|argument| &argument.value)
            .chain(self.capturing_callee(callee))
            .collect();
        if stored.is_empty() {
            return self.held_by(&call.ty, scope);
        }
        ResultScope::Outliving(
            stored
                .into_iter()
                .flat_map(|place| self.dropped_before_place(place))
                .collect(),
        )
    }

    fn capturing_callee<'c>(&self, callee: &'c TypedExpr) -> Option<&'c TypedExpr> {
        match callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(called))
                if !self.local_functions.contains(&called) =>
            {
                Some(callee)
            }
            _ => None,
        }
    }

    fn passes_mut(argument: &TypedArgument) -> bool {
        argument.borrow == Borrow::Mutable || matches!(argument.value.ty, Type::Mut(..))
    }

    fn dropped_before_place(&self, place: &TypedExpr) -> Vec<BindingId> {
        let place_depth = place
            .place_root()
            .and_then(|root| {
                self.in_scope
                    .iter()
                    .rev()
                    .find(|scoped| scoped.binding == root)
            })
            .map_or(ScopedBinding::OUTSIDE_DEFINITION, |scoped| scoped.depth);
        self.in_scope
            .iter()
            .filter(|scoped| scoped.dropped_before(place_depth))
            .map(|scoped| scoped.binding)
            .collect()
    }

    fn record(&mut self, callee: &TypedExpr, arguments: &'e [TypedArgument], scope: &ResultScope) {
        let TypedExprKind::Leaf(TypedLeaf::Local(called)) = callee.kind else {
            return;
        };
        self.calls.push(ScopedCall {
            called,
            arguments,
            scope: scope.clone(),
        });
    }
}
