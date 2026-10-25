use std::collections::{HashMap, HashSet};

use super::mutation::ChangedValues;
use super::problems::OwnershipProblems;
use crate::core::{BindingArena, BindingId};
use crate::diagnostic::Diagnostic;
use crate::source::Span;
use crate::types::{
    Borrow, ClosureKind, CopyRules, GenericId, GenericParam, InferredCopies, TemplateBorrows, Type,
    TypedArgument, TypedBlock, TypedBodyItem, TypedControl, TypedExpr, TypedExprKind, TypedLambda,
    TypedLeaf, TypedStructExpr,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Usage {
    Move,
    Borrow,
}

pub struct MoveReport {
    pub diagnostics: Vec<Diagnostic>,
    pub inferred_copies: InferredCopies,
}

pub struct MoveChecker<'a> {
    copy: CopyRules<'a>,
    bindings: &'a BindingArena,
    template_borrows: &'a TemplateBorrows,
    generics: &'a HashMap<GenericId, GenericParam>,
    enclosing: BindingId,
    changed: ChangedValues,
    moved: HashMap<BindingId, Span>,
    copy_functions: HashSet<BindingId>,
    reported: HashSet<BindingId>,
    parameters: HashSet<BindingId>,
    unwritten_parameters: HashSet<BindingId>,
    inferred_copies: InferredCopies,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> MoveChecker<'a> {
    pub fn new(
        copy: CopyRules<'a>,
        bindings: &'a BindingArena,
        template_borrows: &'a TemplateBorrows,
        generics: &'a HashMap<GenericId, GenericParam>,
        enclosing: BindingId,
    ) -> Self {
        Self {
            copy,
            bindings,
            template_borrows,
            generics,
            enclosing,
            changed: ChangedValues::default(),
            moved: HashMap::new(),
            copy_functions: HashSet::new(),
            reported: HashSet::new(),
            parameters: HashSet::new(),
            unwritten_parameters: HashSet::new(),
            inferred_copies: InferredCopies::default(),
            diagnostics: Vec::new(),
        }
    }

    pub fn check(mut self, lambda: &TypedLambda) -> MoveReport {
        self.parameters = lambda.params.iter().map(|param| param.binding).collect();
        self.unwritten_parameters = lambda
            .params
            .iter()
            .filter(|param| !param.annotated)
            .map(|param| param.binding)
            .collect();
        self.changed = ChangedValues::of(lambda, self.enclosing);
        self.block(&lambda.body, Usage::Move);
        MoveReport {
            diagnostics: self.diagnostics,
            inferred_copies: self.inferred_copies,
        }
    }

    fn expr(&mut self, expr: &TypedExpr, usage: Usage) {
        match &expr.kind {
            TypedExprKind::Leaf(leaf) => self.leaf(leaf, expr, usage),
            TypedExprKind::Call(callee, arguments) => self.call(callee, arguments),
            TypedExprKind::Control(control) => self.control(control, usage),
            other => self.nested_expr(expr, other),
        }
    }

    fn nested_expr(&mut self, expr: &TypedExpr, kind: &TypedExprKind) {
        match kind {
            TypedExprKind::Recur(arguments) => self.recur(arguments),
            TypedExprKind::Struct(structure) => self.structure(structure),
            TypedExprKind::Variant(_, arguments) => self.expressions(arguments),
            TypedExprKind::Assoc(value, keys, item) => self.assoc(value, keys, item),
            _ => self.value_kind(expr, kind),
        }
    }

    fn value_kind(&mut self, expr: &TypedExpr, kind: &TypedExprKind) {
        match kind {
            TypedExprKind::Try { .. } | TypedExprKind::Borrow(..) | TypedExprKind::Deref(..) => {
                self.written(expr)
            }
            TypedExprKind::Fn(lambda) => {
                self.closure(lambda);
            }
            _ => unreachable!("the caller matches a nested expression"),
        }
    }

    fn expressions(&mut self, arguments: &[TypedExpr]) {
        arguments
            .iter()
            .for_each(|argument| self.expr(argument, Usage::Move));
    }

    fn assoc(&mut self, value: &TypedExpr, keys: &[TypedExpr], item: &TypedExpr) {
        self.expr(value, Usage::Move);
        self.expressions(keys);
        self.expr(item, Usage::Move);
    }

    fn leaf(&mut self, leaf: &TypedLeaf, expr: &TypedExpr, usage: Usage) {
        if let TypedLeaf::Local(binding) = leaf {
            self.local(*binding, &expr.ty, expr.span, usage);
        }
    }

    fn written(&mut self, expr: &TypedExpr) {
        match &expr.kind {
            TypedExprKind::Borrow(_, value) | TypedExprKind::Deref(value) => {
                self.expr(value, Usage::Borrow)
            }
            TypedExprKind::Try { value, .. } => self.expr(value, Usage::Move),
            _ => unreachable!("the caller matches a written form"),
        }
    }

    fn structure(&mut self, structure: &TypedStructExpr) {
        match structure {
            TypedStructExpr::Field(value, _) => self.expr(value, Usage::Borrow),
            TypedStructExpr::New(_, fields) => fields
                .iter()
                .for_each(|(_, value)| self.expr(value, Usage::Move)),
            TypedStructExpr::Update(_, base, fields) => {
                self.expr(base, Usage::Move);
                fields
                    .iter()
                    .for_each(|(_, value)| self.expr(value, Usage::Move));
            }
        }
    }

    fn control(&mut self, control: &TypedControl, usage: Usage) {
        match control {
            TypedControl::Block(block) => self.block(block, usage),
            TypedControl::If(test, then, otherwise) => self.branches(test, then, otherwise, usage),
            TypedControl::Match(matched) => {
                self.expr(&matched.value, Usage::Move);
                let before = self.moved.clone();
                let mut arm_moves = Vec::new();
                for arm in &matched.arms {
                    self.moved.clone_from(&before);
                    self.block(&arm.body, usage);
                    arm_moves.push(self.moved.clone());
                }
                self.moved = before;
                for moved in arm_moves {
                    self.moved.extend(moved);
                }
            }
        }
    }

    fn closure(&mut self, lambda: &TypedLambda) -> bool {
        self.block(&lambda.body, Usage::Move);
        lambda.kind == ClosureKind::Fn
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
        for (position, argument) in arguments.iter().enumerate() {
            let usage = if self.moves(callee, position, argument) {
                Usage::Move
            } else {
                Usage::Borrow
            };
            self.expr(&argument.value, usage);
        }
    }

    fn moves(&self, callee: &TypedExpr, position: usize, argument: &TypedArgument) -> bool {
        !self.template_borrows.borrows(callee, position)
            && argument.borrow == Borrow::None
            && !matches!(argument.value.ty, Type::Ref(_, _))
    }

    fn recur(&mut self, arguments: &[TypedArgument]) {
        for argument in arguments {
            let usage = if argument.borrow == Borrow::None
                && !matches!(argument.value.ty, Type::Ref(_, _))
            {
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
                TypedBodyItem::Let(binder, value) => self.bind(binder.binding, value),
                TypedBodyItem::Expr(expr) if Some(position) == last => self.expr(expr, usage),
                TypedBodyItem::Expr(expr) => self.expr(expr, Usage::Move),
            }
        }
    }

    fn local(&mut self, binding: BindingId, ty: &Type, span: Span, usage: Usage) {
        match self.moved.get(&binding).copied() {
            Some(moved_at) => self.use_after_move(binding, ty, span, moved_at),
            None => self.record_use(binding, ty, span, usage),
        }
    }

    fn use_after_move(&mut self, binding: BindingId, ty: &Type, span: Span, moved_at: Span) {
        if self.records_inferred_copy(binding, ty) || !self.reported.insert(binding) {
            return;
        }
        let diagnostic = self.use_after_move_diagnostic(binding, ty, span, moved_at);
        self.diagnostics.push(diagnostic);
    }

    fn use_after_move_diagnostic(
        &self,
        binding: BindingId,
        ty: &Type,
        span: Span,
        moved_at: Span,
    ) -> Diagnostic {
        let name = self.bindings.get(binding).name();
        if let Some(diagnostic) = self.bound_help(binding, ty, span, moved_at) {
            return diagnostic;
        }
        let moved = OwnershipProblems::use_after_move(name, span, moved_at);
        self.ownership_help(binding, ty, &moved)
            .unwrap_or_else(|| OwnershipProblems::clone_suggestion(moved, name, moved_at))
    }

    fn bound_help(
        &self,
        binding: BindingId,
        ty: &Type,
        span: Span,
        moved_at: Span,
    ) -> Option<Diagnostic> {
        let name = self.bindings.get(binding).name();
        if matches!(ty, Type::Function(..)) {
            return Some(OwnershipProblems::closure_move_help(
                OwnershipProblems::use_after_move(name, span, moved_at),
            ));
        }
        let Type::Generic(generic) = ty else {
            return None;
        };
        if !self.parameters.contains(&binding) {
            return None;
        }
        let parameter = self.generics.get(generic)?;
        Some(OwnershipProblems::missing_copy_bound(
            name,
            &parameter.name,
            self.copy.copy_trait_name(self.bindings),
            span,
            moved_at,
        ))
    }

    fn ownership_help(
        &self,
        binding: BindingId,
        ty: &Type,
        moved: &Diagnostic,
    ) -> Option<Diagnostic> {
        if matches!(ty, Type::Mut(_, _)) {
            return Some(OwnershipProblems::mut_parameter_move_help(moved.clone()));
        }
        if self.changed.is_changed(binding) {
            return Some(OwnershipProblems::changed_value_help(moved.clone()));
        }
        if self.parameters.contains(&binding) && self.changed.is_recursive_argument(binding) {
            return Some(OwnershipProblems::recursive_parameter_help(moved.clone()));
        }
        None
    }

    fn record_use(&mut self, binding: BindingId, ty: &Type, span: Span, usage: Usage) {
        let is_copy = self.copy.is_copy(ty) || self.copy_functions.contains(&binding);
        if usage == Usage::Move && !is_copy {
            self.moved.insert(binding, span);
        }
    }

    fn records_inferred_copy(&mut self, binding: BindingId, ty: &Type) -> bool {
        let copies_parameter =
            self.unwritten_parameters.contains(&binding) && matches!(ty, Type::Generic(_));
        if copies_parameter {
            self.inferred_copies.insert(binding);
        }
        copies_parameter
    }
}
