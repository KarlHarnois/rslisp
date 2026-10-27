use std::collections::{HashMap, HashSet};

use super::plan::EmitPlan;
use crate::core::BindingId;
use crate::types::{
    Borrow, Type, TypedArgument, TypedBinder, TypedBlock, TypedBodyItem, TypedControl, TypedExpr,
    TypedExprKind, TypedLambda, TypedLeaf, TypedMatch, TypedProgram,
};

pub struct ResultBorrows<'a> {
    program: &'a TypedProgram,
    plan: &'a EmitPlan<'a>,
    parameters: HashSet<BindingId>,
    aliases: HashMap<BindingId, HashSet<BindingId>>,
    enclosing: HashSet<BindingId>,
}

impl<'a> ResultBorrows<'a> {
    pub fn of(
        program: &'a TypedProgram,
        plan: &'a EmitPlan<'a>,
        lambda: &TypedLambda,
    ) -> HashSet<BindingId> {
        Self::within(program, plan, lambda, HashSet::new())
    }

    fn within(
        program: &'a TypedProgram,
        plan: &'a EmitPlan<'a>,
        lambda: &TypedLambda,
        enclosing: HashSet<BindingId>,
    ) -> HashSet<BindingId> {
        let mut borrows = Self {
            program,
            plan,
            parameters: lambda.params.iter().map(|param| param.binding).collect(),
            aliases: HashMap::new(),
            enclosing,
        };
        let mut flows = borrows.block(&lambda.body);
        let local_borrow_callees: HashSet<BindingId> = LocalBorrowCalls::of(program, lambda)
            .into_iter()
            .flat_map(|callee| borrows.local(callee))
            .collect();
        flows.retain(|binding| !local_borrow_callees.contains(binding));
        flows
    }

    fn block(&mut self, block: &TypedBlock) -> HashSet<BindingId> {
        for item in &block.items {
            if let TypedBodyItem::Let(binder, value) = item {
                let flows = self.bound(value);
                self.aliases.insert(binder.binding, flows);
            }
        }
        match block.items.last() {
            Some(TypedBodyItem::Expr(expr)) => self.value(expr),
            _ => HashSet::new(),
        }
    }

    fn bound(&mut self, value: &TypedExpr) -> HashSet<BindingId> {
        match value.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => self.local(binding),
            _ => self.value(value),
        }
    }

    fn value(&mut self, expr: &TypedExpr) -> HashSet<BindingId> {
        if !self.carries_borrow(&expr.ty) {
            return HashSet::new();
        }
        match &expr.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => self.local(*binding),
            TypedExprKind::Control(control) => self.control(control),
            TypedExprKind::Call(callee, arguments) => self.call(callee, arguments, expr),
            _ => self.children(expr),
        }
    }

    fn carries_borrow(&self, ty: &Type) -> bool {
        self.program.type_holds_borrow(ty) || ty.is_unsized()
    }

    fn call(
        &mut self,
        callee: &TypedExpr,
        arguments: &[TypedArgument],
        call: &TypedExpr,
    ) -> HashSet<BindingId> {
        let mut flows = self.children(call);
        flows.extend(self.callee(callee, arguments));
        flows
    }

    fn callee(&mut self, callee: &TypedExpr, arguments: &[TypedArgument]) -> HashSet<BindingId> {
        match callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => self.local(binding),
            TypedExprKind::Leaf(TypedLeaf::Global(binding)) => self.passed_on(binding, arguments),
            _ => HashSet::new(),
        }
    }

    fn passed_on(&mut self, callee: BindingId, arguments: &[TypedArgument]) -> HashSet<BindingId> {
        let sharing = self.sharing_parameters(callee);
        arguments
            .iter()
            .zip(sharing)
            .filter(|(_, shares)| *shares)
            .flat_map(|(argument, _)| self.bound(&argument.value))
            .collect()
    }

    fn sharing_parameters(&self, callee: BindingId) -> Vec<bool> {
        let Some(lambda) = self.callee_lambda(callee) else {
            return Vec::new();
        };
        let mut enclosing = self.enclosing.clone();
        enclosing.insert(callee);
        let flows = Self::within(self.program, self.plan, lambda, enclosing);
        lambda
            .params
            .iter()
            .map(|param| flows.contains(&param.binding))
            .collect()
    }

    fn callee_lambda(&self, callee: BindingId) -> Option<&'a TypedLambda> {
        if self.enclosing.contains(&callee) {
            return None;
        }
        match &self.plan.definition(callee)?.value.kind {
            TypedExprKind::Fn(lambda) => Some(lambda),
            _ => None,
        }
    }

    fn control(&mut self, control: &TypedControl) -> HashSet<BindingId> {
        match control {
            TypedControl::Block(block) => self.block(block),
            TypedControl::If(_, then, otherwise) => self.branches(then, otherwise),
            TypedControl::Match(matched) => self.matched(matched),
        }
    }

    fn branches(&mut self, then: &TypedExpr, otherwise: &TypedExpr) -> HashSet<BindingId> {
        let mut flows = self.value(then);
        flows.extend(self.value(otherwise));
        flows
    }

    fn matched(&mut self, matched: &TypedMatch) -> HashSet<BindingId> {
        let mut flows = self.value(&matched.value);
        for arm in &matched.arms {
            flows.extend(self.block(&arm.body));
        }
        flows
    }

    fn children(&mut self, expr: &TypedExpr) -> HashSet<BindingId> {
        expr.children()
            .into_iter()
            .flat_map(|child| self.value(child))
            .collect()
    }

    fn local(&self, binding: BindingId) -> HashSet<BindingId> {
        if self.parameters.contains(&binding) {
            HashSet::from([binding])
        } else {
            self.aliases.get(&binding).cloned().unwrap_or_default()
        }
    }
}

struct LocalBorrowCalls<'a> {
    program: &'a TypedProgram,
    outliving: HashSet<BindingId>,
    callees: HashSet<BindingId>,
}

impl<'a> LocalBorrowCalls<'a> {
    fn of(program: &'a TypedProgram, lambda: &TypedLambda) -> HashSet<BindingId> {
        let outliving = lambda
            .params
            .iter()
            .filter(|param| program.type_holds_borrow(&param.ty))
            .map(|param| param.binding)
            .collect();
        let mut calls = Self {
            program,
            outliving,
            callees: HashSet::new(),
        };
        calls.block(&lambda.body);
        calls.callees
    }

    fn block(&mut self, block: &TypedBlock) {
        for item in &block.items {
            match item {
                TypedBodyItem::Let(binder, value) => self.binding(binder, value),
                TypedBodyItem::Expr(expr) => self.expr(expr),
            }
        }
    }

    fn binding(&mut self, binder: &TypedBinder, value: &TypedExpr) {
        self.expr(value);
        if self.program.type_holds_borrow(&binder.ty) && self.outlives_body(value) {
            self.outliving.insert(binder.binding);
        }
    }

    fn expr(&mut self, expr: &TypedExpr) {
        if let TypedExprKind::Call(callee, arguments) = &expr.kind {
            self.call(callee, arguments);
        }
        match &expr.kind {
            TypedExprKind::Fn(lambda) => self.block(&lambda.body),
            TypedExprKind::Control(TypedControl::Block(block)) => self.block(block),
            TypedExprKind::Control(TypedControl::Match(matched)) => self.matched(matched),
            _ => expr
                .children()
                .into_iter()
                .for_each(|child| self.expr(child)),
        }
    }

    fn call(&mut self, callee: &TypedExpr, arguments: &[TypedArgument]) {
        if let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = callee.kind
            && !self.arguments_outlive_body(arguments)
        {
            self.callees.insert(binding);
        }
    }

    fn matched(&mut self, matched: &TypedMatch) {
        self.expr(&matched.value);
        for arm in &matched.arms {
            self.block(&arm.body);
        }
    }

    fn arguments_outlive_body(&self, arguments: &[TypedArgument]) -> bool {
        arguments
            .iter()
            .all(|argument| self.argument_outlives_body(argument))
    }

    fn argument_outlives_body(&self, argument: &TypedArgument) -> bool {
        if self.program.type_holds_borrow(&argument.value.ty) {
            self.outlives_body(&argument.value)
        } else {
            argument.borrow == Borrow::None
        }
    }

    fn outlives_body(&self, expr: &TypedExpr) -> bool {
        match &expr.kind {
            TypedExprKind::Leaf(TypedLeaf::Literal(_) | TypedLeaf::Global(_)) => true,
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => self.outliving.contains(binding),
            TypedExprKind::Call(_, arguments) => self.arguments_outlive_body(arguments),
            TypedExprKind::Control(control) => self.control_outlives_body(control),
            _ => false,
        }
    }

    fn control_outlives_body(&self, control: &TypedControl) -> bool {
        match control {
            TypedControl::If(_, then, otherwise) => {
                self.outlives_body(then) && self.outlives_body(otherwise)
            }
            TypedControl::Match(matched) => matched
                .arms
                .iter()
                .all(|arm| self.block_outlives_body(&arm.body)),
            TypedControl::Block(block) => self.block_outlives_body(block),
        }
    }

    fn block_outlives_body(&self, block: &TypedBlock) -> bool {
        matches!(block.items.last(), Some(TypedBodyItem::Expr(tail)) if self.outlives_body(tail))
    }
}
