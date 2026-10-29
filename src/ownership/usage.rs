use std::collections::{HashMap, HashSet};

use super::functions::{FunctionTable, ParamMode};
use crate::core::{BindingId, ReferenceKind};
use crate::types::{
    Borrow, CopyRules, Type, TypedArgument, TypedBlock, TypedBodyItem, TypedControl, TypedExpr,
    TypedExprKind, TypedLambda, TypedLeaf, TypedPattern, TypedStructExpr,
};

pub struct UsageCollector<'a> {
    functions: &'a FunctionTable,
    modes: &'a HashMap<BindingId, ParamMode>,
    copy: &'a CopyRules<'a>,
    tracked: HashSet<BindingId>,
    pattern_bindings: HashSet<BindingId>,
    self_modes: Vec<ParamMode>,
    required: HashMap<BindingId, ParamMode>,
    pattern_required: HashMap<BindingId, ParamMode>,
}

impl<'a> UsageCollector<'a> {
    pub fn new(
        functions: &'a FunctionTable,
        modes: &'a HashMap<BindingId, ParamMode>,
        copy: &'a CopyRules<'a>,
        tracked: HashSet<BindingId>,
        self_modes: Vec<ParamMode>,
    ) -> Self {
        Self {
            functions,
            modes,
            copy,
            tracked,
            pattern_bindings: HashSet::new(),
            self_modes,
            required: HashMap::new(),
            pattern_required: HashMap::new(),
        }
    }

    pub fn collect(mut self, body: &TypedBlock) -> HashMap<BindingId, ParamMode> {
        self.block(body, ParamMode::Own);
        self.required
    }

    fn expr(&mut self, expr: &TypedExpr, context: ParamMode) {
        match &expr.kind {
            TypedExprKind::Leaf(leaf) => self.leaf(leaf, context),
            TypedExprKind::Call(callee, arguments) => self.call(callee, arguments),
            TypedExprKind::Control(control) => self.control(control, context),
            other => self.nested_expr(expr, other, context),
        }
    }

    fn nested_expr(&mut self, expr: &TypedExpr, kind: &TypedExprKind, context: ParamMode) {
        match kind {
            TypedExprKind::Recur(arguments) => self.recur(arguments),
            TypedExprKind::Struct(structure) => self.structure(structure, context),
            TypedExprKind::Variant(_, arguments) => self.expressions(arguments),
            TypedExprKind::Assoc(value, keys, item) => self.assoc(value, keys, item),
            TypedExprKind::TraitObject(value, _) => self.expr(value, context),
            _ => self.value_kind(expr, kind),
        }
    }

    fn value_kind(&mut self, expr: &TypedExpr, kind: &TypedExprKind) {
        match kind {
            TypedExprKind::Try { .. } | TypedExprKind::Borrow(..) | TypedExprKind::Deref(..) => {
                self.written(expr)
            }
            TypedExprKind::Fn(lambda) => self.closure_body(lambda),
            _ => unreachable!("the caller matches a nested expression"),
        }
    }

    fn expressions(&mut self, arguments: &[TypedExpr]) {
        arguments
            .iter()
            .for_each(|argument| self.expr(argument, ParamMode::Own));
    }

    fn assoc(&mut self, value: &TypedExpr, keys: &[TypedExpr], item: &TypedExpr) {
        self.expr(value, ParamMode::Own);
        self.expressions(keys);
        self.expr(item, ParamMode::Own);
    }

    fn leaf(&mut self, leaf: &TypedLeaf, context: ParamMode) {
        if let TypedLeaf::Local(binding) = leaf {
            self.require(*binding, context);
        }
    }

    fn written(&mut self, expr: &TypedExpr) {
        match &expr.kind {
            TypedExprKind::Borrow(ReferenceKind::Shared, value) => self.expr(value, ParamMode::Ref),
            TypedExprKind::Borrow(ReferenceKind::Mutable, value) => {
                self.expr(value, ParamMode::Mut)
            }
            TypedExprKind::Deref(value) => self.expr(value, ParamMode::Ref),
            TypedExprKind::Try { value, .. } => self.expr(value, ParamMode::Own),
            _ => unreachable!("the caller matches a written form"),
        }
    }

    fn closure_body(&mut self, lambda: &TypedLambda) {
        let outer = std::mem::take(&mut self.self_modes);
        self.block(&lambda.body, ParamMode::Own);
        self.self_modes = outer;
    }

    fn structure(&mut self, structure: &TypedStructExpr, context: ParamMode) {
        match structure {
            TypedStructExpr::Field(value, _) => self.expr(value, Self::field_context(context)),
            TypedStructExpr::New(_, fields) => fields
                .iter()
                .for_each(|(_, value)| self.expr(value, ParamMode::Own)),
            TypedStructExpr::Update(_, base, fields) => {
                self.update_base(base);
                fields
                    .iter()
                    .for_each(|(_, value)| self.expr(value, ParamMode::Own));
            }
        }
    }

    fn update_base(&mut self, base: &TypedExpr) {
        match &base.kind {
            TypedExprKind::Struct(TypedStructExpr::Field(owner, _)) => self.update_base(owner),
            _ => self.expr(base, ParamMode::Own),
        }
    }

    fn field_context(context: ParamMode) -> ParamMode {
        match context {
            ParamMode::Mut => ParamMode::Mut,
            ParamMode::Copy | ParamMode::Ref | ParamMode::Own => ParamMode::Ref,
        }
    }

    fn control(&mut self, control: &TypedControl, context: ParamMode) {
        match control {
            TypedControl::Block(block) => self.block(block, context),
            TypedControl::If(test, then, otherwise) => {
                self.expr(test, ParamMode::Copy);
                self.expr(then, context);
                self.expr(otherwise, context);
            }
            TypedControl::Match(matched) => {
                let binders: HashMap<BindingId, Type> = matched
                    .arms
                    .iter()
                    .flat_map(|arm| Self::pattern_bindings(&arm.pattern))
                    .collect();
                self.pattern_bindings.extend(binders.keys().copied());
                for arm in &matched.arms {
                    self.block(&arm.body, context);
                }
                let moves = binders
                    .iter()
                    .any(|(binding, ty)| self.binding_needs_move(*binding, ty));
                self.expr(
                    &matched.value,
                    if moves {
                        ParamMode::Own
                    } else {
                        ParamMode::Ref
                    },
                );
            }
        }
    }

    fn block(&mut self, block: &TypedBlock, context: ParamMode) {
        let last = block.items.len().checked_sub(1);
        for (position, item) in block.items.iter().enumerate() {
            match item {
                TypedBodyItem::Let(_, value) => self.expr(value, ParamMode::Own),
                TypedBodyItem::Expr(expr) if Some(position) == last => self.expr(expr, context),
                TypedBodyItem::Expr(expr) => self.expr(expr, ParamMode::Own),
            }
        }
    }

    fn call(&mut self, callee: &TypedExpr, arguments: &[TypedArgument]) {
        let callee_modes = self.callee_modes(callee);
        if !matches!(
            callee.kind,
            TypedExprKind::Leaf(TypedLeaf::Global(_)) | TypedExprKind::Leaf(TypedLeaf::Rust(_))
        ) {
            self.expr(callee, ParamMode::Ref);
        }
        self.arguments(arguments, &callee_modes);
    }

    fn recur(&mut self, arguments: &[TypedArgument]) {
        let modes = self.self_modes.clone();
        self.arguments(arguments, &modes);
    }

    fn arguments(&mut self, arguments: &[TypedArgument], modes: &[ParamMode]) {
        for (position, argument) in arguments.iter().enumerate() {
            let context = match argument.borrow {
                Borrow::Shared | Borrow::Passthrough | Borrow::Deref(_) | Borrow::Collapse(_) => {
                    ParamMode::Ref
                }
                Borrow::Mutable | Borrow::Reborrow => ParamMode::Mut,
                Borrow::None => modes
                    .get(position)
                    .copied()
                    .map_or(ParamMode::Own, Self::argument_context),
                Borrow::Pending(_) => {
                    unreachable!("the type checker decides every pending borrow")
                }
            };
            self.expr(&argument.value, context);
        }
    }

    fn callee_modes(&self, callee: &TypedExpr) -> Vec<ParamMode> {
        let TypedExprKind::Leaf(TypedLeaf::Global(binding)) = callee.kind else {
            return Vec::new();
        };
        self.functions
            .get(binding)
            .map(|function| {
                function
                    .params
                    .iter()
                    .map(|slot| {
                        self.modes
                            .get(&slot.binding)
                            .copied()
                            .unwrap_or(ParamMode::Own)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn argument_context(callee_mode: ParamMode) -> ParamMode {
        match callee_mode {
            ParamMode::Ref | ParamMode::Mut => callee_mode,
            ParamMode::Copy | ParamMode::Own => ParamMode::Own,
        }
    }

    fn pattern_bindings(pattern: &TypedPattern) -> Vec<(BindingId, Type)> {
        pattern
            .binders()
            .into_iter()
            .map(|binder| (binder.binding, binder.ty.clone()))
            .collect()
    }

    fn binding_needs_move(&self, binding: BindingId, ty: &Type) -> bool {
        match self
            .pattern_required
            .get(&binding)
            .copied()
            .unwrap_or(ParamMode::Copy)
        {
            ParamMode::Copy | ParamMode::Ref => false,
            ParamMode::Mut => true,
            ParamMode::Own => !self.copy.is_copy(ty),
        }
    }

    fn require(&mut self, binding: BindingId, mode: ParamMode) {
        if self.tracked.contains(&binding) {
            let required = self.required.entry(binding).or_insert(ParamMode::Copy);
            *required = (*required).max(mode);
        } else if self.pattern_bindings.contains(&binding) {
            let required = self
                .pattern_required
                .entry(binding)
                .or_insert(ParamMode::Copy);
            *required = (*required).max(mode);
        }
    }
}
