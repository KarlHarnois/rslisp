use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use super::callee_parameters::{CalleeParameters, OpenCallees};
use super::plan::EmitPlan;
use crate::core::BindingId;
use crate::types::{
    Borrow, Type, TypedArgument, TypedBinder, TypedBlock, TypedBodyItem, TypedControl, TypedExpr,
    TypedExprKind, TypedLambda, TypedLeaf, TypedMatch, TypedProgram,
};

pub struct ResultBorrows<'a> {
    program: &'a TypedProgram,
    plan: &'a EmitPlan<'a>,
    parameters_by_callee: RefCell<HashMap<BindingId, CalleeParameters>>,
    open_callees: RefCell<OpenCallees>,
}

struct Visit {
    parameters: CalleeParameters,
    earliest_reached: usize,
}

impl<'a> ResultBorrows<'a> {
    pub fn new(program: &'a TypedProgram, plan: &'a EmitPlan<'a>) -> Self {
        Self {
            program,
            plan,
            parameters_by_callee: RefCell::new(HashMap::new()),
            open_callees: RefCell::new(OpenCallees::default()),
        }
    }

    pub fn analyse(&self, lambda: &TypedLambda) -> LambdaFlows {
        BodyFlows::of(self, lambda, OpenCallees::NONE_REACHED)
    }

    fn cached_parameters(&self, callee: BindingId) -> Option<CalleeParameters> {
        self.parameters_by_callee.borrow().get(&callee).cloned()
    }

    fn open_order(&self, callee: BindingId) -> Option<usize> {
        self.open_callees.borrow().order(callee)
    }

    fn provisional_parameters(&self, callee: BindingId) -> CalleeParameters {
        self.open_callees.borrow_mut().assumed(callee)
    }

    fn visit(&self, callee: BindingId, lambda: &TypedLambda) -> Visit {
        let order = self.open_callees.borrow_mut().open(callee);
        let visit = self.proposed_visit(callee, lambda, order);
        if visit.earliest_reached == order {
            self.settle_group(order);
            return self.settled_visit(callee);
        }
        visit
    }

    fn proposed_visit(&self, callee: BindingId, lambda: &TypedLambda, order: usize) -> Visit {
        let analysed = BodyFlows::of(self, lambda, order);
        let parameters = analysed.parameters(lambda);
        self.open_callees
            .borrow_mut()
            .propose(callee, parameters.clone());
        Visit {
            parameters,
            earliest_reached: analysed.earliest_reached,
        }
    }

    fn settled_visit(&self, callee: BindingId) -> Visit {
        Visit {
            parameters: self.cached_parameters(callee).unwrap_or_default(),
            earliest_reached: OpenCallees::NONE_REACHED,
        }
    }

    fn settle_group(&self, order: usize) {
        let group = self.open_callees.borrow().group_from(order);
        if self.open_callees.borrow().any_assumed(&group) {
            self.reach_fixed_point(&group);
        }
        let settled = self.open_callees.borrow_mut().close(&group);
        self.parameters_by_callee.borrow_mut().extend(settled);
    }

    fn reach_fixed_point(&self, group: &[BindingId]) {
        while self.widen_group(group) {}
    }

    fn widen_group(&self, group: &[BindingId]) -> bool {
        let grown: Vec<bool> = group
            .iter()
            .map(|callee| self.widen_member(*callee))
            .collect();
        grown.contains(&true)
    }

    fn widen_member(&self, callee: BindingId) -> bool {
        let Some(lambda) = self.callee_lambda(callee) else {
            return false;
        };
        let analysed = BodyFlows::of(self, lambda, OpenCallees::NONE_REACHED);
        let parameters = analysed.parameters(lambda);
        self.open_callees.borrow_mut().widen(callee, &parameters)
    }

    fn callee_lambda(&self, callee: BindingId) -> Option<&'a TypedLambda> {
        match &self.plan.definition(callee)?.value.kind {
            TypedExprKind::Fn(lambda) => Some(lambda),
            _ => None,
        }
    }
}

struct BodyFlows<'r, 'a> {
    rules: &'r ResultBorrows<'a>,
    parameters: HashSet<BindingId>,
    aliases: HashMap<BindingId, HashSet<BindingId>>,
    earliest_reached: usize,
}

pub struct LambdaFlows {
    pub flows: HashSet<BindingId>,
    pub higher_ranked: HashSet<BindingId>,
    earliest_reached: usize,
}

impl LambdaFlows {
    fn parameters(&self, lambda: &TypedLambda) -> CalleeParameters {
        CalleeParameters {
            sharing: Self::marked(lambda, &self.flows),
            higher_ranked: Self::marked(lambda, &self.higher_ranked),
        }
    }

    fn marked(lambda: &TypedLambda, bindings: &HashSet<BindingId>) -> Vec<bool> {
        lambda
            .params
            .iter()
            .map(|param| bindings.contains(&param.binding))
            .collect()
    }
}

impl<'r, 'a> BodyFlows<'r, 'a> {
    fn of(rules: &'r ResultBorrows<'a>, lambda: &TypedLambda, order: usize) -> LambdaFlows {
        let mut borrows = Self {
            rules,
            parameters: lambda.params.iter().map(|param| param.binding).collect(),
            aliases: HashMap::new(),
            earliest_reached: order,
        };
        let mut flows = borrows.block(&lambda.body);
        let calls = LocalBorrowCalls::of(rules.program, lambda);
        let mut higher_ranked: HashSet<BindingId> = calls
            .callees
            .iter()
            .flat_map(|callee| borrows.local(*callee))
            .collect();
        flows.retain(|binding| !higher_ranked.contains(binding));
        higher_ranked.extend(borrows.passed_to_higher_ranked(&calls.passed));
        LambdaFlows {
            flows,
            higher_ranked,
            earliest_reached: borrows.earliest_reached,
        }
    }

    fn passed_to_higher_ranked(&mut self, passed: &[PassedLocal]) -> HashSet<BindingId> {
        let higher_ranked: Vec<&PassedLocal> = passed
            .iter()
            .filter(|passed| self.reaches_higher_ranked(passed))
            .collect();
        higher_ranked
            .into_iter()
            .flat_map(|passed| self.local(passed.local))
            .collect()
    }

    fn reaches_higher_ranked(&mut self, passed: &PassedLocal) -> bool {
        self.callee_parameters(passed.callee)
            .higher_ranked
            .get(passed.position)
            .is_some_and(|higher_ranked| *higher_ranked)
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
        self.rules.program.type_holds_borrow(ty) || ty.is_unsized()
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
        let sharing = self.callee_parameters(callee).sharing;
        arguments
            .iter()
            .zip(sharing)
            .filter(|(_, shares)| *shares)
            .flat_map(|(argument, _)| self.bound(&argument.value))
            .collect()
    }

    fn callee_parameters(&mut self, callee: BindingId) -> CalleeParameters {
        if let Some(cached) = self.rules.cached_parameters(callee) {
            return cached;
        }
        match self.rules.open_order(callee) {
            Some(order) => self.assumed_parameters(callee, order),
            None => self.visited_parameters(callee),
        }
    }

    fn assumed_parameters(&mut self, callee: BindingId, order: usize) -> CalleeParameters {
        self.reach(order);
        self.rules.provisional_parameters(callee)
    }

    fn visited_parameters(&mut self, callee: BindingId) -> CalleeParameters {
        let Some(lambda) = self.rules.callee_lambda(callee) else {
            return CalleeParameters::default();
        };
        let visit = self.rules.visit(callee, lambda);
        self.reach(visit.earliest_reached);
        visit.parameters
    }

    fn reach(&mut self, order: usize) {
        self.earliest_reached = self.earliest_reached.min(order);
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

struct PassedLocal {
    callee: BindingId,
    position: usize,
    local: BindingId,
}

struct LocalBorrowCalls<'a> {
    program: &'a TypedProgram,
    outliving: HashSet<BindingId>,
    callees: HashSet<BindingId>,
    passed: Vec<PassedLocal>,
}

impl<'a> LocalBorrowCalls<'a> {
    fn of(program: &'a TypedProgram, lambda: &TypedLambda) -> Self {
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
            passed: Vec::new(),
        };
        calls.block(&lambda.body);
        calls
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
        match callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding))
                if !self.arguments_outlive_body(arguments) =>
            {
                self.callees.insert(binding);
            }
            TypedExprKind::Leaf(TypedLeaf::Global(binding)) => self.pass(binding, arguments),
            _ => {}
        }
    }

    fn pass(&mut self, callee: BindingId, arguments: &[TypedArgument]) {
        for (position, argument) in arguments.iter().enumerate() {
            if let TypedExprKind::Leaf(TypedLeaf::Local(local)) = argument.value.kind
                && self.returns_argument_borrow(&argument.value.ty)
            {
                self.passed.push(PassedLocal {
                    callee,
                    position,
                    local,
                });
            }
        }
    }

    fn returns_argument_borrow(&self, ty: &Type) -> bool {
        matches!(ty, Type::Function(_, parameters, result) if self.program.function_type_returns_argument_borrow(parameters, result))
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
