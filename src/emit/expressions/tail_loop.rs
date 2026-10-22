use std::collections::HashSet;

use super::{ArgumentRole, ExpressionEmitter, TypeContext};
use crate::core::BindingId;
use crate::emit::fragment::Fragment;
use crate::types::{
    Type, TypedArgument, TypedBlock, TypedBodyItem, TypedControl, TypedExpr, TypedExprKind,
    TypedLambda, TypedLeaf,
};

pub struct TailLoop {
    function: BindingId,
    params: Vec<BindingId>,
    reassigned: HashSet<BindingId>,
}

impl TailLoop {
    pub fn of(function: BindingId, lambda: &TypedLambda) -> Option<Self> {
        let params: Vec<BindingId> = lambda.params.iter().map(|param| param.binding).collect();
        let mut calls = Vec::new();
        Self::collect_block(function, &lambda.body, &mut calls);
        if calls.is_empty() {
            return None;
        }
        let reassigned = calls
            .iter()
            .flat_map(|arguments| {
                params
                    .iter()
                    .zip(arguments.iter())
                    .filter(|(param, argument)| !Self::passes_through(**param, argument))
                    .map(|(param, _)| *param)
            })
            .collect();
        Some(Self {
            function,
            params,
            reassigned,
        })
    }

    pub fn reassigns(&self, param: BindingId) -> bool {
        self.reassigned.contains(&param)
    }

    fn collect_block<'e>(
        function: BindingId,
        block: &'e TypedBlock,
        calls: &mut Vec<&'e [TypedArgument]>,
    ) {
        if let Some(TypedBodyItem::Expr(tail)) = block.items.last() {
            Self::collect(function, tail, calls);
        }
    }

    fn collect<'e>(function: BindingId, expr: &'e TypedExpr, calls: &mut Vec<&'e [TypedArgument]>) {
        match &expr.kind {
            TypedExprKind::Call(callee, arguments) if Self::calls(function, callee) => {
                calls.push(arguments);
            }
            TypedExprKind::Control(TypedControl::If(_, then, otherwise)) => {
                Self::collect(function, then, calls);
                Self::collect(function, otherwise, calls);
            }
            TypedExprKind::Control(TypedControl::Block(block)) => {
                Self::collect_block(function, block, calls);
            }
            _ => {}
        }
    }

    fn calls(function: BindingId, callee: &TypedExpr) -> bool {
        matches!(callee.kind, TypedExprKind::Leaf(TypedLeaf::Global(binding)) if binding == function)
    }

    fn passes_through(param: BindingId, argument: &TypedArgument) -> bool {
        matches!(argument.value.kind, TypedExprKind::Leaf(TypedLeaf::Local(binding)) if binding == param)
    }
}

impl ExpressionEmitter<'_> {
    pub(super) fn loop_body(&mut self, block: &TypedBlock) -> Vec<Fragment> {
        vec![
            Fragment::Newline,
            Fragment::Text("loop {".to_string()),
            Fragment::Indented(self.loop_block(block)),
            Fragment::Newline,
            Fragment::Text("}".to_string()),
        ]
    }

    fn loop_block(&mut self, block: &TypedBlock) -> Vec<Fragment> {
        let Some((TypedBodyItem::Expr(tail), leading)) = block.items.split_last() else {
            let mut fragments = self.leading_statements(&block.items);
            fragments.push(Fragment::Newline);
            fragments.push(Fragment::Text("return;".to_string()));
            return fragments;
        };
        let mut fragments = self.leading_statements(leading);
        fragments.extend(self.loop_tail(tail));
        fragments
    }

    fn leading_statements(&mut self, items: &[TypedBodyItem]) -> Vec<Fragment> {
        let mut fragments = Vec::new();
        for item in items {
            fragments.push(Fragment::Newline);
            fragments.extend(self.statement(item, false).fragments);
        }
        fragments
    }

    fn loop_tail(&mut self, tail: &TypedExpr) -> Vec<Fragment> {
        match &tail.kind {
            TypedExprKind::Call(callee, arguments) if self.calls_itself(callee) => {
                self.next_iteration(arguments)
            }
            TypedExprKind::Control(TypedControl::If(test, then, otherwise)) => {
                self.loop_conditional(test, then, otherwise)
            }
            TypedExprKind::Control(TypedControl::Block(_)) => self.loop_branch(tail),
            _ => self.loop_return(tail),
        }
    }

    fn loop_branch(&mut self, branch: &TypedExpr) -> Vec<Fragment> {
        let TypedExprKind::Control(TypedControl::Block(block)) = &branch.kind else {
            return self.loop_tail(branch);
        };
        self.locals.enter();
        let fragments = self.loop_block(block);
        self.locals.leave();
        fragments
    }

    fn loop_conditional(
        &mut self,
        test: &TypedExpr,
        then: &TypedExpr,
        otherwise: &TypedExpr,
    ) -> Vec<Fragment> {
        let test = self.expr(test);
        let mut fragments = vec![Fragment::Newline, Fragment::Text("if ".to_string())];
        fragments.extend(test.fragments);
        fragments.push(Fragment::Text(" {".to_string()));
        fragments.push(Fragment::Indented(self.loop_branch(then)));
        fragments.push(Fragment::Newline);
        fragments.push(Fragment::Text("} else {".to_string()));
        fragments.push(Fragment::Indented(self.loop_branch(otherwise)));
        fragments.push(Fragment::Newline);
        fragments.push(Fragment::Text("}".to_string()));
        fragments
    }

    fn loop_return(&mut self, tail: &TypedExpr) -> Vec<Fragment> {
        let value = self.expr(tail);
        let mut fragments = vec![Fragment::Newline];
        if tail.ty == Type::unit() {
            fragments.extend(value.fragments);
            fragments.push(Fragment::Text(";".to_string()));
            fragments.push(Fragment::Newline);
            fragments.push(Fragment::Text("return;".to_string()));
        } else {
            fragments.push(Fragment::Text("return ".to_string()));
            fragments.extend(value.fragments);
            fragments.push(Fragment::Text(";".to_string()));
        }
        fragments
    }

    fn calls_itself(&self, callee: &TypedExpr) -> bool {
        self.tail_loop
            .as_ref()
            .is_some_and(|tail_loop| TailLoop::calls(tail_loop.function, callee))
    }

    fn next_iteration(&mut self, arguments: &[TypedArgument]) -> Vec<Fragment> {
        let params = self
            .tail_loop
            .as_ref()
            .map(|tail_loop| tail_loop.params.clone())
            .unwrap_or_default();
        let mut names = Vec::new();
        let mut values = Vec::new();
        for (param, argument) in params.into_iter().zip(arguments) {
            if TailLoop::passes_through(param, argument) {
                continue;
            }
            names.push(self.locals.get(param).unwrap_or("_").to_string());
            values.push(self.argument(argument, TypeContext::Known, ArgumentRole::Ordinary));
        }
        let mut fragments = Vec::new();
        if !names.is_empty() {
            let (open, close) = if names.len() == 1 {
                ("", "")
            } else {
                ("(", ")")
            };
            fragments.push(Fragment::Newline);
            fragments.push(Fragment::Text(format!(
                "{open}{}{close} = {open}",
                names.join(", ")
            )));
            for (position, value) in values.into_iter().enumerate() {
                if position > 0 {
                    fragments.push(Fragment::Text(", ".to_string()));
                }
                fragments.extend(value.fragments);
            }
            fragments.push(Fragment::Text(format!("{close};")));
        }
        fragments.push(Fragment::Newline);
        fragments.push(Fragment::Text("continue;".to_string()));
        fragments
    }
}
