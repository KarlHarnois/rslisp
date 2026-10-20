use std::collections::{HashMap, HashSet};

use super::functions::{FunctionTable, ParamMode};
use crate::core::BindingId;
use crate::types::{Borrow, TypedArgument, TypedBlock, TypedBodyItem, TypedExpr, TypedExprKind};

pub struct UsageCollector<'a> {
    functions: &'a FunctionTable,
    modes: &'a HashMap<BindingId, ParamMode>,
    tracked: HashSet<BindingId>,
    required: HashMap<BindingId, ParamMode>,
}

impl<'a> UsageCollector<'a> {
    pub fn new(
        functions: &'a FunctionTable,
        modes: &'a HashMap<BindingId, ParamMode>,
        tracked: HashSet<BindingId>,
    ) -> Self {
        Self {
            functions,
            modes,
            tracked,
            required: HashMap::new(),
        }
    }

    pub fn collect(mut self, body: &TypedBlock) -> HashMap<BindingId, ParamMode> {
        self.block(body, ParamMode::Own);
        self.required
    }

    fn expr(&mut self, expr: &TypedExpr, context: ParamMode) {
        match &expr.kind {
            TypedExprKind::Local(binding) => self.require(*binding, context),
            TypedExprKind::Call(callee, arguments) => self.call(callee, arguments),
            TypedExprKind::Field(value, _) => self.expr(value, ParamMode::Ref),
            TypedExprKind::New(_, fields) => fields
                .iter()
                .for_each(|(_, value)| self.expr(value, ParamMode::Own)),
            TypedExprKind::Block(block) => self.block(block, context),
            TypedExprKind::If(test, then, otherwise) => {
                self.expr(test, ParamMode::Copy);
                self.expr(then, context);
                self.expr(otherwise, context);
            }
            TypedExprKind::Fn(lambda) => self.block(&lambda.body, ParamMode::Own),
            TypedExprKind::Literal(_) | TypedExprKind::Global(_) | TypedExprKind::Rust(_) => {}
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
            TypedExprKind::Global(_) | TypedExprKind::Rust(_)
        ) {
            self.expr(callee, ParamMode::Ref);
        }
        for (position, argument) in arguments.iter().enumerate() {
            let context = match argument.borrow {
                Borrow::Shared | Borrow::Passthrough | Borrow::Deref => ParamMode::Ref,
                Borrow::Mutable => ParamMode::Mut,
                Borrow::None => callee_modes
                    .get(position)
                    .copied()
                    .map_or(ParamMode::Own, Self::argument_context),
            };
            self.expr(&argument.value, context);
        }
    }

    fn callee_modes(&self, callee: &TypedExpr) -> Vec<ParamMode> {
        let TypedExprKind::Global(binding) = callee.kind else {
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

    fn require(&mut self, binding: BindingId, mode: ParamMode) {
        if self.tracked.contains(&binding) {
            let required = self.required.entry(binding).or_insert(ParamMode::Copy);
            *required = (*required).max(mode);
        }
    }
}
