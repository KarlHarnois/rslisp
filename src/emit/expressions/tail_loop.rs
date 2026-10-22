use std::collections::HashSet;

use super::{ArgumentRole, ExpressionEmitter, TypeContext};
use crate::core::BindingId;
use crate::emit::fragment::Fragment;
use crate::types::{
    Type, TypedArgument, TypedBlock, TypedBodyItem, TypedControl, TypedExpr, TypedExprKind,
    TypedLambda, TypedLeaf,
};

pub struct TailLoop {
    function: Option<BindingId>,
    params: Vec<BindingId>,
    reassigned: HashSet<BindingId>,
}

impl TailLoop {
    pub fn of(function: BindingId, lambda: &TypedLambda) -> Option<Self> {
        Self::prepare(Some(function), lambda)
    }

    pub fn of_closure(lambda: &TypedLambda) -> Option<Self> {
        Self::prepare(None, lambda)
    }

    fn prepare(function: Option<BindingId>, lambda: &TypedLambda) -> Option<Self> {
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

    pub fn recurrence<'e>(&self, expr: &'e TypedExpr) -> Option<&'e [TypedArgument]> {
        match &expr.kind {
            TypedExprKind::Recur(arguments) => Some(arguments),
            TypedExprKind::Call(callee, arguments) if self.calls(callee) => Some(arguments),
            _ => None,
        }
    }

    fn collect_block<'e>(
        function: Option<BindingId>,
        block: &'e TypedBlock,
        calls: &mut Vec<&'e [TypedArgument]>,
    ) {
        if let Some(TypedBodyItem::Expr(tail)) = block.items.last() {
            Self::collect(function, tail, calls);
        }
    }

    fn collect<'e>(
        function: Option<BindingId>,
        expr: &'e TypedExpr,
        calls: &mut Vec<&'e [TypedArgument]>,
    ) {
        match &expr.kind {
            TypedExprKind::Recur(arguments) => {
                calls.push(arguments);
            }
            TypedExprKind::Call(callee, arguments) if Self::calls_global(function, callee) => {
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

    fn calls(&self, callee: &TypedExpr) -> bool {
        Self::calls_global(self.function, callee)
    }

    fn calls_global(function: Option<BindingId>, callee: &TypedExpr) -> bool {
        let Some(function) = function else {
            return false;
        };
        matches!(callee.kind, TypedExprKind::Leaf(TypedLeaf::Global(binding)) if binding == function)
    }

    fn passes_through(param: BindingId, argument: &TypedArgument) -> bool {
        matches!(argument.value.kind, TypedExprKind::Leaf(TypedLeaf::Local(binding)) if binding == param)
    }
}

impl ExpressionEmitter<'_> {
    const COMPOUND_OPERATORS: [&'static str; 5] = ["+", "-", "*", "/", "%"];

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
        if let Some(arguments) = self
            .tail_loop
            .as_ref()
            .and_then(|tail_loop| tail_loop.recurrence(tail))
        {
            return self.next_iteration(arguments);
        }
        match &tail.kind {
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

    fn next_iteration(&mut self, arguments: &[TypedArgument]) -> Vec<Fragment> {
        let params = self
            .tail_loop
            .as_ref()
            .map(|tail_loop| tail_loop.params.clone())
            .unwrap_or_default();
        let changed: Vec<(BindingId, &TypedArgument)> = params
            .into_iter()
            .zip(arguments)
            .filter(|(param, argument)| !TailLoop::passes_through(*param, argument))
            .collect();
        let mut fragments = match changed.as_slice() {
            [] => Vec::new(),
            [(param, argument)] => self.single_assignment(*param, argument),
            _ => self.tuple_assignment(&changed),
        };
        fragments.push(Fragment::Newline);
        fragments.push(Fragment::Text("continue;".to_string()));
        fragments
    }

    fn single_assignment(&mut self, param: BindingId, argument: &TypedArgument) -> Vec<Fragment> {
        let name = self.parameter_name(param);
        let (operator, value) = match self.compound_operand(param, argument) {
            Some((operator, operand)) => (format!("{operator}="), operand),
            None => ("=".to_string(), argument),
        };
        let mut fragments = vec![
            Fragment::Newline,
            Fragment::Text(format!("{name} {operator} ")),
        ];
        fragments.extend(
            self.argument(value, TypeContext::Known, ArgumentRole::Ordinary)
                .fragments,
        );
        fragments.push(Fragment::Text(";".to_string()));
        fragments
    }

    fn tuple_assignment(&mut self, changed: &[(BindingId, &TypedArgument)]) -> Vec<Fragment> {
        let names: Vec<String> = changed
            .iter()
            .map(|(param, _)| self.parameter_name(*param))
            .collect();
        let mut fragments = vec![
            Fragment::Newline,
            Fragment::Text(format!("({}) = (", names.join(", "))),
        ];
        for (position, (_, argument)) in changed.iter().enumerate() {
            if position > 0 {
                fragments.push(Fragment::Text(", ".to_string()));
            }
            fragments.extend(
                self.argument(argument, TypeContext::Known, ArgumentRole::Ordinary)
                    .fragments,
            );
        }
        fragments.push(Fragment::Text(");".to_string()));
        fragments
    }

    fn parameter_name(&self, param: BindingId) -> String {
        self.locals.get(param).unwrap_or("_").to_string()
    }

    fn compound_operand<'e>(
        &self,
        param: BindingId,
        argument: &'e TypedArgument,
    ) -> Option<(&'static str, &'e TypedArgument)> {
        let TypedExprKind::Call(callee, operands) = &argument.value.kind else {
            return None;
        };
        let [target, operand] = operands.as_slice() else {
            return None;
        };
        let operator = self.compound_operator(callee)?;
        matches!(target.value.kind, TypedExprKind::Leaf(TypedLeaf::Local(local)) if local == param)
            .then_some((operator, operand))
    }

    fn compound_operator(&self, callee: &TypedExpr) -> Option<&'static str> {
        let TypedExprKind::Leaf(TypedLeaf::Global(binding)) = callee.kind else {
            return None;
        };
        let TypedExprKind::Leaf(TypedLeaf::Rust(template)) =
            &self.plan.definition(binding)?.value.kind
        else {
            return None;
        };
        Self::COMPOUND_OPERATORS
            .into_iter()
            .find(|operator| *template.template == *format!("{{0}} {operator} {{1}}"))
    }
}
