use std::collections::HashSet;

use super::{ArgumentRole, BoxPosition, ExpressionEmitter, LoopExit, TypeContext};
use crate::core::BindingId;
use crate::emit::fragment::{Code, Fragment};
use crate::types::{
    Type, TypedArgument, TypedArm, TypedBlock, TypedBodyItem, TypedControl, TypedExpr,
    TypedExprKind, TypedLambda, TypedLeaf, TypedMatch,
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
            TypedExprKind::Control(TypedControl::Match(matched)) => {
                for arm in &matched.arms {
                    Self::collect_block(function, &arm.body, calls);
                }
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

    pub(super) fn loop_expression(&mut self, block: &TypedBlock, exit: LoopExit) -> Vec<Fragment> {
        if exit == LoopExit::Break
            && let Some(fragments) = self.while_let_loop(block)
        {
            return fragments;
        }
        let previous = std::mem::replace(&mut self.loop_exit, exit);
        let fragments = vec![
            Fragment::Text("loop {".to_string()),
            Fragment::Indented(self.loop_block(block)),
            Fragment::Newline,
            Fragment::Text("}".to_string()),
        ];
        self.loop_exit = previous;
        fragments
    }

    fn while_let_loop(&mut self, block: &TypedBlock) -> Option<Vec<Fragment>> {
        if !self.tail_loop.as_ref()?.params.is_empty() {
            return None;
        }
        let matched = Self::body_match(block)?;
        self.while_let_match(matched)
    }

    fn body_match(block: &TypedBlock) -> Option<&TypedMatch> {
        let [
            TypedBodyItem::Expr(TypedExpr {
                kind: TypedExprKind::Control(TypedControl::Match(matched)),
                ..
            }),
        ] = block.items.as_slice()
        else {
            return None;
        };
        Some(matched)
    }

    fn while_let_match(&mut self, matched: &TypedMatch) -> Option<Vec<Fragment>> {
        if matched.arms.len() != 2 || !matched.unguarded_exhaustive {
            return None;
        }
        let repeating = self.repeating_arm(matched)?;
        let ending = 1 - repeating;
        if !Self::is_empty_do(&matched.arms[ending].body) {
            return None;
        }
        let arm = &matched.arms[repeating];
        let position = BoxPosition::of(&matched.value.ty);
        let value = self.expr(&matched.value);
        self.locals.enter();
        let pattern = self.pattern(&arm.pattern, position);
        if !pattern.guards.is_empty() || !pattern.prologue.is_empty() {
            self.locals.leave();
            return None;
        }
        let body = self.repeating_body(arm);
        self.locals.leave();
        Some(Self::while_let_fragments(pattern.code, value, body))
    }

    fn repeating_arm(&self, matched: &TypedMatch) -> Option<usize> {
        match matched
            .arms
            .iter()
            .map(|arm| self.repeats(arm))
            .collect::<Vec<_>>()
            .as_slice()
        {
            [true, false] => Some(0),
            [false, true] => Some(1),
            _ => None,
        }
    }

    fn repeats(&self, arm: &TypedArm) -> bool {
        self.recurring_block(arm).is_some()
    }

    fn recurring_block<'e>(&self, arm: &'e TypedArm) -> Option<Option<&'e TypedBlock>> {
        let [TypedBodyItem::Expr(only)] = arm.body.items.as_slice() else {
            return None;
        };
        let TypedExprKind::Control(TypedControl::Block(block)) = &only.kind else {
            return self.recurres(only).then_some(None);
        };
        let Some(TypedBodyItem::Expr(tail)) = block.items.last() else {
            return None;
        };
        self.recurres(tail).then_some(Some(block))
    }

    fn recurres(&self, tail: &TypedExpr) -> bool {
        self.tail_loop
            .as_ref()
            .is_some_and(|tail_loop| tail_loop.recurrence(tail).is_some())
    }

    fn is_empty_do(body: &TypedBlock) -> bool {
        matches!(
            body.items.as_slice(),
            [TypedBodyItem::Expr(TypedExpr {
                kind: TypedExprKind::Control(TypedControl::Block(block)),
                ..
            })] if block.items.is_empty()
        )
    }

    fn repeating_body(&mut self, arm: &TypedArm) -> Vec<Fragment> {
        let Some(Some(block)) = self.recurring_block(arm) else {
            return Vec::new();
        };
        let leading = block.items.len().saturating_sub(1);
        self.leading_statements(&block.items[..leading])
    }

    fn while_let_fragments(pattern: Code, value: Code, body: Vec<Fragment>) -> Vec<Fragment> {
        let mut fragments = vec![Fragment::Text("while let ".to_string())];
        fragments.extend(pattern.fragments);
        fragments.push(Fragment::Text(" = ".to_string()));
        fragments.extend(value.fragments);
        fragments.push(Fragment::Text(" {".to_string()));
        fragments.push(Fragment::Indented(body));
        fragments.push(Fragment::Newline);
        fragments.push(Fragment::Text("}".to_string()));
        fragments
    }

    fn loop_block(&mut self, block: &TypedBlock) -> Vec<Fragment> {
        let Some((TypedBodyItem::Expr(tail), leading)) = block.items.split_last() else {
            let mut fragments = self.leading_statements(&block.items);
            fragments.push(Fragment::Newline);
            fragments.push(Fragment::Text(format!("{};", self.loop_exit.keyword())));
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
                let mut fragments = vec![Fragment::Newline];
                fragments.extend(self.loop_conditional(test, then, otherwise));
                fragments
            }
            TypedExprKind::Control(TypedControl::Block(_)) => self.loop_branch(tail),
            TypedExprKind::Control(TypedControl::Match(matched)) => {
                let mut fragments = vec![Fragment::Newline];
                fragments.extend(self.loop_match(matched));
                fragments
            }
            _ => self.loop_return(tail),
        }
    }

    fn loop_match(&mut self, matched: &TypedMatch) -> Vec<Fragment> {
        if let Some(arm) = self.sole_sure_arm(&matched.arms) {
            return self.loop_sure_arm(&matched.value, arm);
        }
        let value = self.expr(&matched.value);
        let mut fragments = vec![Fragment::Text("match ".to_string())];
        fragments.extend(value.fragments);
        fragments.push(Fragment::Text(" {".to_string()));
        let position = BoxPosition::of(&matched.value.ty);
        let mut inner = Vec::new();
        for arm in &matched.arms {
            inner.push(Fragment::Newline);
            self.locals.enter();
            let (head, prologue) = self.arm_head(&arm.pattern, position);
            inner.extend(head);
            inner.push(Fragment::Text(" => {".to_string()));
            let body = self.loop_block(&arm.body);
            inner.push(Fragment::Indented(Self::with_prologue(body, &prologue)));
            inner.push(Fragment::Newline);
            inner.push(Fragment::Text("}".to_string()));
            self.locals.leave();
        }
        Self::add_fallback(&mut inner, matched.unguarded_exhaustive);
        fragments.push(Fragment::Indented(inner));
        fragments.push(Fragment::Newline);
        fragments.push(Fragment::Text("}".to_string()));
        fragments
    }

    fn loop_sure_arm(&mut self, value: &TypedExpr, arm: &TypedArm) -> Vec<Fragment> {
        let position = BoxPosition::of(&value.ty);
        let value = self.expr(value);
        self.locals.enter();
        let pattern = self.pattern(&arm.pattern, position);
        let mut fragments = vec![Fragment::Text("let ".to_string())];
        fragments.extend(pattern.code.fragments);
        fragments.push(Fragment::Text(" = ".to_string()));
        fragments.extend(value.fragments);
        fragments.push(Fragment::Text(";".to_string()));
        let body = self.loop_block(&arm.body);
        fragments.extend(Self::with_prologue(body, &pattern.prologue));
        self.locals.leave();
        fragments
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
        let mut fragments = vec![Fragment::Text("if ".to_string())];
        fragments.extend(test.fragments);
        fragments.push(Fragment::Text(" {".to_string()));
        fragments.push(Fragment::Indented(self.loop_branch(then)));
        fragments.push(Fragment::Newline);
        if let TypedExprKind::Control(TypedControl::If(next_test, next_then, next_otherwise)) =
            &otherwise.kind
        {
            fragments.push(Fragment::Text("} else ".to_string()));
            fragments.extend(self.loop_conditional(next_test, next_then, next_otherwise));
        } else {
            fragments.push(Fragment::Text("} else {".to_string()));
            fragments.push(Fragment::Indented(self.loop_branch(otherwise)));
            fragments.push(Fragment::Newline);
            fragments.push(Fragment::Text("}".to_string()));
        }
        fragments
    }

    fn loop_return(&mut self, tail: &TypedExpr) -> Vec<Fragment> {
        let value = self.expr(tail);
        let mut fragments = vec![Fragment::Newline];
        if tail.ty == Type::unit() {
            fragments.extend(value.fragments);
            fragments.push(Fragment::Text(";".to_string()));
            fragments.push(Fragment::Newline);
            fragments.push(Fragment::Text(format!("{};", self.loop_exit.keyword())));
        } else {
            fragments.push(Fragment::Text(format!("{} ", self.loop_exit.keyword())));
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
