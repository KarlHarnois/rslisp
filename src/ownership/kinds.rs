use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use super::aliases::LetAliases;
use super::definitions::LocalDefinitions;
use super::lambda_address::LambdaAddress;
use super::parameter_uses::ParameterUses;
use super::problems::OwnershipProblems;
use super::subtree::Subtree;
use super::update_bases::UpdateBases;
use crate::core::{BindingArena, BindingId, FieldLabel, ReferenceKind};
use crate::diagnostic::Diagnostic;
use crate::scope::FastHashMap;
use crate::source::Span;
use crate::types::{
    Borrow, ClosureKind, CopyRules, FieldPlace, StructTable, TemplateBorrows, Type, TypedArgument,
    TypedBinder, TypedBlock, TypedBodyItem, TypedControl, TypedExpr, TypedExprKind, TypedLambda,
    TypedLeaf, TypedModule, TypedProgram, TypedStructExpr,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Usage {
    Move,
    Borrow,
}

struct LocalClosure {
    binding: BindingId,
    body_uses: ParameterUses,
}

impl LocalClosure {
    fn bound_by(item: &TypedBodyItem) -> Option<BindingId> {
        match item {
            TypedBodyItem::Let(binder, value) if matches!(value.kind, TypedExprKind::Fn(_)) => {
                Some(binder.binding)
            }
            _ => None,
        }
    }

    fn repeat_body_if_called_again(self, uses: &mut ParameterUses) {
        if !uses.allows_fn_once(self.binding) {
            uses.extend(self.body_uses);
        }
    }
}

#[derive(Clone, Default)]
struct CaptureReport {
    moved: BTreeMap<BindingId, Span>,
    changed: BTreeMap<BindingId, Span>,
}

impl CaptureReport {
    fn merge(&mut self, other: Self) {
        for (binding, span) in other.moved {
            self.moved.entry(binding).or_insert(span);
        }
        for (binding, span) in other.changed {
            self.changed.entry(binding).or_insert(span);
        }
    }

    fn outside(&self, own: &HashSet<BindingId>) -> Self {
        Self {
            moved: self.moved_outside(own),
            changed: self.changed_outside(own),
        }
    }

    fn moved_outside(&self, own: &HashSet<BindingId>) -> BTreeMap<BindingId, Span> {
        self.moved
            .iter()
            .filter(|(binding, _)| !own.contains(binding))
            .map(|(binding, span)| (*binding, *span))
            .collect()
    }

    fn changed_outside(&self, own: &HashSet<BindingId>) -> BTreeMap<BindingId, Span> {
        self.changed
            .iter()
            .filter(|(binding, _)| !own.contains(binding))
            .map(|(binding, span)| (*binding, *span))
            .collect()
    }

    fn kind(&self) -> ClosureKind {
        if !self.moved.is_empty() {
            ClosureKind::FnOnce
        } else if !self.changed.is_empty() {
            ClosureKind::FnMut
        } else {
            ClosureKind::Fn
        }
    }
}

pub struct KindReport {
    pub mutable_bindings: HashSet<BindingId>,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct KindSources<'a> {
    pub traits: &'a crate::types::TraitTable,
    pub generics: &'a HashMap<crate::types::GenericId, crate::types::GenericParam>,
    pub structs: &'a StructTable,
    pub enums: &'a crate::types::EnumTable,
    pub bindings: &'a BindingArena,
    pub template_borrows: &'a TemplateBorrows,
    pub function_parameters: HashMap<BindingId, Vec<BindingId>>,
    pub rust_parameter_kinds: HashMap<BindingId, Vec<ClosureKind>>,
    pub written_parameter_kinds: HashMap<BindingId, ClosureKind>,
    pub borrowing_lambdas: &'a HashMap<BindingId, HashSet<BindingId>>,
}

pub struct ClosureKinds<'a> {
    copy: CopyRules<'a>,
    structs: &'a crate::types::StructTable,
    bindings: &'a BindingArena,
    template_borrows: &'a TemplateBorrows,
    parameter_kinds: HashMap<BindingId, ClosureKind>,
    binding_kinds: HashMap<BindingId, ClosureKind>,
    binding_captures: HashMap<BindingId, BindingId>,
    value_parameters: HashMap<BindingId, Vec<BindingId>>,
    borrowing_lambdas: &'a HashMap<BindingId, HashSet<BindingId>>,
    function_parameters: HashMap<BindingId, Vec<BindingId>>,
    pass_edges: HashMap<BindingId, HashSet<BindingId>>,
    cycle_peers: HashMap<BindingId, Vec<BindingId>>,
    rust_parameter_kinds: HashMap<BindingId, Vec<ClosureKind>>,
    written_parameter_kinds: HashMap<BindingId, ClosureKind>,
    result_kinds: HashMap<BindingId, ClosureKind>,
    alias_origins: HashMap<BindingId, BindingId>,
    definitions: LocalDefinitions,
    reusable_reports: FastHashMap<LambdaAddress, CaptureReport>,
    registered_function_bindings: usize,
}

impl<'a> ClosureKinds<'a> {
    const STRONGEST_CALLABLE_BORROW: ClosureKind = ClosureKind::FnMut;

    pub fn function_parameters(program: &TypedProgram) -> HashMap<BindingId, Vec<BindingId>> {
        program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .filter_map(|definition| match &definition.value.kind {
                TypedExprKind::Fn(lambda) => Some((
                    definition.binding,
                    lambda.params.iter().map(|param| param.binding).collect(),
                )),
                _ => None,
            })
            .collect()
    }

    pub fn written_parameter_kinds(program: &TypedProgram) -> HashMap<BindingId, ClosureKind> {
        program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .filter_map(|definition| match &definition.value.kind {
                TypedExprKind::Fn(lambda) => Some(&lambda.params),
                _ => None,
            })
            .flatten()
            .filter_map(|param| match &param.ty {
                Type::Function(kind, _, _) if param.annotated && *kind != ClosureKind::Fn => {
                    Some((param.binding, *kind))
                }
                _ => None,
            })
            .collect()
    }

    pub fn rust_parameter_kinds(program: &TypedProgram) -> HashMap<BindingId, Vec<ClosureKind>> {
        program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .filter_map(|definition| {
                if !matches!(
                    definition.value.kind,
                    TypedExprKind::Leaf(TypedLeaf::Rust(_))
                ) {
                    return None;
                }
                let Type::Function(_, parameters, _) = &definition.scheme.body else {
                    return None;
                };
                let kinds = parameters
                    .iter()
                    .map(|parameter| match parameter {
                        Type::Function(kind, _, _) => *kind,
                        _ => ClosureKind::Fn,
                    })
                    .collect();
                Some((definition.binding, kinds))
            })
            .collect()
    }

    pub fn new(sources: KindSources<'a>) -> Self {
        Self {
            copy: CopyRules::new(
                sources.traits,
                sources.generics,
                sources.structs,
                sources.enums,
            ),
            structs: sources.structs,
            bindings: sources.bindings,
            template_borrows: sources.template_borrows,
            parameter_kinds: HashMap::new(),
            binding_kinds: HashMap::new(),
            binding_captures: HashMap::new(),
            value_parameters: HashMap::new(),
            borrowing_lambdas: sources.borrowing_lambdas,
            function_parameters: sources.function_parameters,
            pass_edges: HashMap::new(),
            cycle_peers: HashMap::new(),
            rust_parameter_kinds: sources.rust_parameter_kinds,
            written_parameter_kinds: sources.written_parameter_kinds,
            result_kinds: HashMap::new(),
            alias_origins: HashMap::new(),
            definitions: LocalDefinitions::default(),
            reusable_reports: FastHashMap::default(),
            registered_function_bindings: 0,
        }
    }

    fn build_alias_origins(&mut self, modules: &[TypedModule]) {
        let expressions: Vec<&TypedExpr> = modules
            .iter()
            .flat_map(|module| &module.definitions)
            .filter(|definition| Self::takes_function(&definition.value.ty))
            .flat_map(|definition| Subtree::of(&definition.value))
            .collect();
        self.alias_origins = LetAliases::in_expressions(&expressions).origins();
    }

    fn takes_function(ty: &Type) -> bool {
        matches!(ty, Type::Function(_, parameters, _)
            if parameters.iter().any(|parameter| matches!(parameter, Type::Function(..))))
    }

    fn build_pass_edges(&mut self, modules: &[TypedModule]) {
        for definition in modules.iter().flat_map(|module| &module.definitions) {
            self.build_lambda_pass_edges(&definition.value);
        }
    }

    fn build_lambda_pass_edges(&mut self, expr: &TypedExpr) {
        if let TypedExprKind::Fn(lambda) = &expr.kind {
            let params: HashSet<BindingId> = lambda
                .params
                .iter()
                .filter(|param| matches!(param.ty, Type::Function(..)))
                .map(|param| param.binding)
                .collect();
            for child in lambda.body.expressions().filter(|_| !params.is_empty()) {
                self.build_body_pass_edges(child, &params);
            }
        }
        for child in expr.children() {
            self.build_lambda_pass_edges(child);
        }
    }

    fn build_body_pass_edges(&mut self, expr: &TypedExpr, params: &HashSet<BindingId>) {
        if let TypedExprKind::Call(callee, arguments) = &expr.kind
            && let TypedExprKind::Leaf(TypedLeaf::Global(function)) = &callee.kind
            && let Some(parameters) = self.function_parameters.get(function)
        {
            for (position, argument) in arguments.iter().enumerate() {
                let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = &argument.value.kind else {
                    continue;
                };
                let Some(target) = parameters.get(position) else {
                    continue;
                };
                if params.contains(binding) {
                    self.pass_edges.entry(*binding).or_default().insert(*target);
                }
            }
        }
        for child in expr.children() {
            self.build_body_pass_edges(child, params);
        }
    }

    pub fn run(mut self, modules: &mut [TypedModule]) -> KindReport {
        self.build_alias_origins(modules);
        self.build_pass_edges(modules);
        self.build_cycle_peers();
        loop {
            let known_binding_kinds = self.binding_kinds.clone();
            self.recompute_lambdas(modules);
            self.recompute_result_kinds(modules);
            let kinds = self.recompute_parameter_kinds(modules);
            if kinds == self.parameter_kinds && self.binding_kinds == known_binding_kinds {
                break;
            }
            self.parameter_kinds = kinds;
        }
        let diagnostics = self.check_calls(modules);
        let mutable_bindings = self.called_mutable_bindings(modules);
        self.patch_types(modules);
        KindReport {
            mutable_bindings,
            diagnostics,
        }
    }

    fn recompute_lambdas(&mut self, modules: &mut [TypedModule]) {
        self.binding_captures.clear();
        self.value_parameters.clear();
        for definition in modules
            .iter_mut()
            .flat_map(|module| module.definitions.iter_mut())
        {
            self.patch_expression(&mut definition.value);
            self.forget_lambda_analyses();
        }
    }

    fn forget_lambda_analyses(&mut self) {
        self.definitions.forget();
        self.reusable_reports.clear();
    }

    fn patch_expression(&mut self, expr: &mut TypedExpr) {
        if let TypedExprKind::Fn(lambda) = &mut expr.kind {
            let lambda = Rc::make_mut(lambda);
            for child in lambda.body.expressions_mut() {
                self.patch_expression(child);
            }
            let report = self.analyze_lambda(lambda);
            lambda.kind = report.kind();
            lambda.moved_capture = report
                .moved
                .first_key_value()
                .map(|(binding, span)| (*binding, *span));
            lambda.changed_capture = report
                .changed
                .first_key_value()
                .map(|(binding, span)| (*binding, *span));
            if let Type::Function(kind, _, _) = &mut expr.ty {
                *kind = lambda.kind;
            }
            return;
        }
        for child in expr.children_mut() {
            self.patch_expression(child);
        }
    }

    fn analyze_lambda(&mut self, lambda: &TypedLambda) -> CaptureReport {
        let registered_before = self.registered_function_bindings;
        let report = self.scan_lambda_body(lambda);
        if self.registered_function_bindings == registered_before {
            self.reusable_reports
                .insert(LambdaAddress::of(lambda), report.clone());
        }
        report
    }

    fn scan_lambda_body(&mut self, lambda: &TypedLambda) -> CaptureReport {
        let own = self.definitions.of(lambda);
        self.scan_block(&lambda.body, &own, Usage::Move)
    }

    fn scan_block(
        &mut self,
        block: &TypedBlock,
        own: &HashSet<BindingId>,
        usage: Usage,
    ) -> CaptureReport {
        let last = block.items.len().checked_sub(1);
        let mut total = CaptureReport::default();
        for (position, item) in block.items.iter().enumerate() {
            match item {
                TypedBodyItem::Let(binder, value) => {
                    total.merge(self.scan_expression(value, own, Usage::Move));
                    self.register_binding(binder.binding, value);
                }
                TypedBodyItem::Expr(expr) if Some(position) == last => {
                    total.merge(self.scan_expression(expr, own, usage));
                }
                TypedBodyItem::Expr(expr) => {
                    total.merge(self.scan_expression(expr, own, Usage::Move));
                }
            }
        }
        total
    }

    fn register_binding(&mut self, binding: BindingId, value: &TypedExpr) {
        if !matches!(value.ty, Type::Function(..)) {
            return;
        }
        self.registered_function_bindings += 1;
        self.binding_kinds.insert(binding, self.value_kind(value));
        if let Some(capture) = self.value_capture(value) {
            self.binding_captures.insert(binding, capture);
        }
        if let Some(parameters) = self.value_parameters_of(value) {
            self.value_parameters.insert(binding, parameters);
        }
    }

    fn scan_expression(
        &mut self,
        expr: &TypedExpr,
        own: &HashSet<BindingId>,
        usage: Usage,
    ) -> CaptureReport {
        match &expr.kind {
            TypedExprKind::Leaf(leaf) => self.scan_leaf(leaf, expr, own, usage),
            TypedExprKind::Fn(lambda) => self.scan_lambda(lambda, own),
            TypedExprKind::Call(callee, arguments) => {
                let mut report = CaptureReport::default();
                self.scan_call(callee, arguments, own, &mut report);
                report
            }
            TypedExprKind::Control(control) => self.scan_control(control, own, usage),
            other => self.scan_nested(expr, other, own),
        }
    }

    fn scan_leaf(
        &mut self,
        leaf: &TypedLeaf,
        expr: &TypedExpr,
        own: &HashSet<BindingId>,
        usage: Usage,
    ) -> CaptureReport {
        let mut report = CaptureReport::default();
        if let TypedLeaf::Local(binding) = leaf
            && !own.contains(binding)
            && usage == Usage::Move
        {
            self.record_move(&mut report, *binding, &expr.ty, expr.span);
        }
        report
    }

    fn scan_lambda(&mut self, lambda: &TypedLambda, own: &HashSet<BindingId>) -> CaptureReport {
        match self.reusable_reports.get(&LambdaAddress::of(lambda)) {
            Some(report) => report.outside(own),
            None => self.scan_lambda_body(lambda).outside(own),
        }
    }

    fn scan_nested(
        &mut self,
        expr: &TypedExpr,
        kind: &TypedExprKind,
        own: &HashSet<BindingId>,
    ) -> CaptureReport {
        match kind {
            TypedExprKind::Recur(arguments) => {
                let mut report = CaptureReport::default();
                self.scan_arguments(None, arguments, own, &mut report);
                report
            }
            TypedExprKind::Try { .. } | TypedExprKind::Borrow(..) | TypedExprKind::Deref(_) => {
                self.scan_written(expr, kind, own)
            }
            TypedExprKind::TraitObject(value, _) => self.scan_expression(value, own, Usage::Move),
            _ => self.scan_value(expr, kind, own),
        }
    }

    fn scan_written(
        &mut self,
        expr: &TypedExpr,
        kind: &TypedExprKind,
        own: &HashSet<BindingId>,
    ) -> CaptureReport {
        match kind {
            TypedExprKind::Try { value, .. } => self.scan_expression(value, own, Usage::Move),
            TypedExprKind::Borrow(reference, value) => {
                self.scan_borrow(expr, *reference, value, own)
            }
            TypedExprKind::Deref(value) => self.scan_expression(value, own, Usage::Borrow),
            _ => unreachable!("the caller matches a written form"),
        }
    }

    fn scan_value(
        &mut self,
        expr: &TypedExpr,
        kind: &TypedExprKind,
        own: &HashSet<BindingId>,
    ) -> CaptureReport {
        match kind {
            TypedExprKind::Struct(structure) => self.scan_structure(expr, structure, own),
            TypedExprKind::Variant(_, arguments) => {
                let values: Vec<&TypedExpr> = arguments.iter().collect();
                self.scan_values(&values, own)
            }
            TypedExprKind::Assoc(value, keys, item) => {
                let mut values: Vec<&TypedExpr> = keys.iter().collect();
                values.push(item);
                self.scan_update(value, &values, own)
            }
            _ => unreachable!("the caller matches a value form"),
        }
    }

    fn scan_borrow(
        &mut self,
        expr: &TypedExpr,
        reference: ReferenceKind,
        value: &TypedExpr,
        own: &HashSet<BindingId>,
    ) -> CaptureReport {
        let mut report = CaptureReport::default();
        if matches!(reference, ReferenceKind::Mutable)
            && let Some(root) = value.place_root()
            && !own.contains(&root)
        {
            report.changed.entry(root).or_insert(expr.span);
        }
        report.merge(self.scan_expression(value, own, Usage::Borrow));
        report
    }

    fn scan_structure(
        &mut self,
        expr: &TypedExpr,
        structure: &TypedStructExpr,
        own: &HashSet<BindingId>,
    ) -> CaptureReport {
        match structure {
            TypedStructExpr::New(_, fields) => {
                let values: Vec<&TypedExpr> = fields.iter().map(|(_, value)| value).collect();
                self.scan_values(&values, own)
            }
            TypedStructExpr::Update(_, base, fields) => {
                self.scan_struct_update(expr, base, fields, own)
            }
            TypedStructExpr::Field(value, _) => self.scan_expression(value, own, Usage::Borrow),
        }
    }

    fn scan_struct_update(
        &mut self,
        update: &TypedExpr,
        base: &TypedExpr,
        fields: &[(FieldLabel, TypedExpr)],
        own: &HashSet<BindingId>,
    ) -> CaptureReport {
        let mut report = match base.field_place() {
            Some(place) => self.record_unlisted_moves(&place, base.span, update, fields, own),
            None => self.scan_expression(base, own, Usage::Move),
        };
        let values: Vec<&TypedExpr> = fields.iter().map(|(_, value)| value).collect();
        report.merge(self.scan_values(&values, own));
        report
    }

    fn record_unlisted_moves(
        &self,
        place: &FieldPlace,
        span: Span,
        update: &TypedExpr,
        fields: &[(FieldLabel, TypedExpr)],
        own: &HashSet<BindingId>,
    ) -> CaptureReport {
        let bases = UpdateBases::new(self.structs, &self.copy);
        if !bases.moves_any(&bases.unlisted_fields(update, fields)) {
            return CaptureReport::default();
        }
        self.record_field_move(place, span, own)
    }

    fn scan_update(
        &mut self,
        base: &TypedExpr,
        values: &[&TypedExpr],
        own: &HashSet<BindingId>,
    ) -> CaptureReport {
        let mut report = self.scan_update_base(base, own);
        report.merge(self.scan_values(values, own));
        report
    }

    fn scan_update_base(&mut self, base: &TypedExpr, own: &HashSet<BindingId>) -> CaptureReport {
        match base.field_place() {
            Some(place) if place.reads_field() && !self.copy.is_copy(&base.ty) => {
                self.record_field_move(&place, base.span, own)
            }
            _ => self.scan_expression(base, own, Usage::Move),
        }
    }

    fn record_field_move(
        &self,
        place: &FieldPlace,
        span: Span,
        own: &HashSet<BindingId>,
    ) -> CaptureReport {
        let mut report = CaptureReport::default();
        if !own.contains(&place.root) {
            self.record_move(&mut report, place.root, place.root_type, span);
        }
        report
    }

    fn scan_values(&mut self, values: &[&TypedExpr], own: &HashSet<BindingId>) -> CaptureReport {
        let mut report = CaptureReport::default();
        for value in values {
            report.merge(self.scan_expression(value, own, Usage::Move));
        }
        report
    }

    fn scan_call(
        &mut self,
        callee: &TypedExpr,
        arguments: &[TypedArgument],
        own: &HashSet<BindingId>,
        report: &mut CaptureReport,
    ) {
        self.scan_callee(callee, own, report);
        self.scan_arguments(Some(callee), arguments, own, report);
    }

    fn scan_callee(
        &mut self,
        callee: &TypedExpr,
        own: &HashSet<BindingId>,
        report: &mut CaptureReport,
    ) {
        let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = &callee.kind else {
            report.merge(self.scan_expression(callee, own, Usage::Borrow));
            return;
        };
        if own.contains(binding) {
            return;
        }
        self.record_call(report, *binding, callee.span);
    }

    fn record_call(&self, report: &mut CaptureReport, binding: BindingId, span: Span) {
        match self.kind_of(binding) {
            ClosureKind::Fn => {}
            ClosureKind::FnMut => {
                report.changed.entry(binding).or_insert(span);
            }
            ClosureKind::FnOnce => {
                report.moved.entry(binding).or_insert(span);
            }
        }
    }

    fn scan_arguments(
        &mut self,
        callee: Option<&TypedExpr>,
        arguments: &[TypedArgument],
        own: &HashSet<BindingId>,
        report: &mut CaptureReport,
    ) {
        for (position, argument) in arguments.iter().enumerate() {
            self.scan_argument(callee, position, argument, own, report);
        }
    }

    fn scan_argument(
        &mut self,
        callee: Option<&TypedExpr>,
        position: usize,
        argument: &TypedArgument,
        own: &HashSet<BindingId>,
        report: &mut CaptureReport,
    ) {
        let reference = self.argument_reference(callee, position, argument);
        if reference == Some(ReferenceKind::Mutable)
            && let Some(root) = argument.value.place_root()
            && !own.contains(&root)
        {
            report.changed.entry(root).or_insert(argument.value.span);
        }
        let usage = if reference.is_none() && !argument.value.ty.is_reference() {
            Usage::Move
        } else {
            Usage::Borrow
        };
        report.merge(self.scan_expression(&argument.value, own, usage));
    }

    fn argument_reference(
        &self,
        callee: Option<&TypedExpr>,
        position: usize,
        argument: &TypedArgument,
    ) -> Option<ReferenceKind> {
        match argument.borrow {
            Borrow::Mutable | Borrow::Reborrow => Some(ReferenceKind::Mutable),
            Borrow::None => {
                callee.and_then(|callee| self.template_borrows.reference(callee, position))
            }
            _ => Some(ReferenceKind::Shared),
        }
    }

    fn scan_control(
        &mut self,
        control: &TypedControl,
        own: &HashSet<BindingId>,
        usage: Usage,
    ) -> CaptureReport {
        match control {
            TypedControl::Block(block) => self.scan_block(block, own, usage),
            TypedControl::If(test, then, otherwise) => {
                self.scan_branches(test, then, otherwise, own, usage)
            }
            TypedControl::Match(matched) => self.scan_match(matched, own, usage),
        }
    }

    fn scan_branches(
        &mut self,
        test: &TypedExpr,
        then: &TypedExpr,
        otherwise: &TypedExpr,
        own: &HashSet<BindingId>,
        usage: Usage,
    ) -> CaptureReport {
        let mut report = self.scan_expression(test, own, Usage::Borrow);
        report.merge(self.scan_expression(then, own, usage));
        report.merge(self.scan_expression(otherwise, own, usage));
        report
    }

    fn scan_match(
        &mut self,
        matched: &crate::types::TypedMatch,
        own: &HashSet<BindingId>,
        usage: Usage,
    ) -> CaptureReport {
        let mut report = self.scan_expression(&matched.value, own, Usage::Move);
        for arm in &matched.arms {
            report.merge(self.scan_block(&arm.body, own, usage));
        }
        report
    }

    fn record_move(&self, report: &mut CaptureReport, binding: BindingId, ty: &Type, span: Span) {
        if !self.is_copy_binding(binding, ty) {
            report.moved.entry(binding).or_insert(span);
        }
    }

    fn is_copy_binding(&self, binding: BindingId, ty: &Type) -> bool {
        if matches!(ty, Type::Function(..)) {
            return self.kind_of(binding) == ClosureKind::Fn;
        }
        self.copy.is_copy(ty)
    }

    fn kind_of(&self, binding: BindingId) -> ClosureKind {
        self.binding_kinds
            .get(&binding)
            .or_else(|| self.parameter_kinds.get(&binding))
            .copied()
            .unwrap_or(ClosureKind::Fn)
    }

    fn value_kind(&self, value: &TypedExpr) -> ClosureKind {
        if !matches!(value.ty, Type::Function(..)) {
            return ClosureKind::Fn;
        }
        self.function_value_kind(value)
    }

    fn function_value_kind(&self, value: &TypedExpr) -> ClosureKind {
        match &value.kind {
            TypedExprKind::Fn(lambda) => lambda.kind,
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => self.kind_of(*binding),
            TypedExprKind::Leaf(_) => ClosureKind::Fn,
            TypedExprKind::Call(callee, _) => self.call_result_kind(callee),
            TypedExprKind::Control(control) => self.control_value_kind(control),
            _ => ClosureKind::Fn,
        }
    }

    fn control_value_kind(&self, control: &TypedControl) -> ClosureKind {
        match control {
            TypedControl::Block(block) => self.block_value_kind(block),
            TypedControl::If(_, then, otherwise) => {
                self.value_kind(then).max(self.value_kind(otherwise))
            }
            TypedControl::Match(matched) => matched
                .arms
                .iter()
                .map(|arm| self.block_value_kind(&arm.body))
                .max()
                .unwrap_or(ClosureKind::Fn),
        }
    }

    fn block_value_kind(&self, block: &TypedBlock) -> ClosureKind {
        match block.items.last() {
            Some(TypedBodyItem::Expr(expr)) => self.value_kind(expr),
            _ => ClosureKind::Fn,
        }
    }

    fn call_result_kind(&self, callee: &TypedExpr) -> ClosureKind {
        match &callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Global(binding)) => self.result_kind(*binding),
            _ => ClosureKind::Fn,
        }
    }

    fn result_kind(&self, binding: BindingId) -> ClosureKind {
        self.result_kinds
            .get(&binding)
            .copied()
            .unwrap_or(ClosureKind::Fn)
    }

    fn value_capture(&self, value: &TypedExpr) -> Option<BindingId> {
        match &value.kind {
            TypedExprKind::Fn(lambda) => Self::lambda_capture(lambda),
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => {
                self.binding_captures.get(binding).copied()
            }
            _ => None,
        }
    }

    fn lambda_capture(lambda: &TypedLambda) -> Option<BindingId> {
        match lambda.kind {
            ClosureKind::Fn => None,
            ClosureKind::FnMut => lambda.changed_capture.map(|(binding, _)| binding),
            ClosureKind::FnOnce => lambda.moved_capture.map(|(binding, _)| binding),
        }
    }

    fn value_parameters_of(&self, value: &TypedExpr) -> Option<Vec<BindingId>> {
        match &value.kind {
            TypedExprKind::Fn(lambda) => {
                Some(lambda.params.iter().map(|param| param.binding).collect())
            }
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => {
                self.value_parameters.get(binding).cloned()
            }
            TypedExprKind::Leaf(TypedLeaf::Global(binding)) => {
                self.function_parameters.get(binding).cloned()
            }
            _ => None,
        }
    }

    fn recompute_result_kinds(&mut self, modules: &[TypedModule]) {
        self.result_kinds.clear();
        for definition in modules.iter().flat_map(|module| &module.definitions) {
            let TypedExprKind::Fn(lambda) = &definition.value.kind else {
                continue;
            };
            let Type::Function(_, _, result) = &definition.value.ty else {
                continue;
            };
            if matches!(result.as_ref(), Type::Function(..)) {
                self.result_kinds
                    .insert(definition.binding, self.block_value_kind(&lambda.body));
            }
        }
    }

    fn recompute_parameter_kinds(
        &self,
        modules: &[TypedModule],
    ) -> HashMap<BindingId, ClosureKind> {
        let mut kinds: HashMap<BindingId, ClosureKind> = modules
            .iter()
            .flat_map(|module| &module.definitions)
            .flat_map(|definition| self.lambdas(&definition.value))
            .flat_map(|lambda| {
                lambda
                    .params
                    .iter()
                    .map(|param| {
                        let kind = self
                            .written_parameter_kinds
                            .get(&param.binding)
                            .copied()
                            .unwrap_or(ClosureKind::Fn);
                        (param.binding, kind)
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        for definition in modules.iter().flat_map(|module| &module.definitions) {
            self.requirements(&definition.value, &mut kinds);
        }
        self.share_cycle_kinds(&mut kinds);
        self.cap_borrowed_function_parameters(&mut kinds);
        kinds
    }

    fn cap_borrowed_function_parameters(&self, kinds: &mut HashMap<BindingId, ClosureKind>) {
        for parameter in self.borrowing_lambdas.values().flatten() {
            if let Some(kind) = kinds.get_mut(parameter) {
                *kind = (*kind).min(Self::STRONGEST_CALLABLE_BORROW);
            }
        }
    }

    fn lambdas(&self, expr: &TypedExpr) -> Vec<Rc<TypedLambda>> {
        let mut found = Vec::new();
        self.collect_lambdas(expr, &mut found);
        found
    }

    fn collect_lambdas(&self, expr: &TypedExpr, found: &mut Vec<Rc<TypedLambda>>) {
        if let TypedExprKind::Fn(lambda) = &expr.kind {
            found.push(Rc::clone(lambda));
            for child in lambda.body.expressions() {
                self.collect_lambdas(child, found);
            }
            return;
        }
        for child in expr.children() {
            self.collect_lambdas(child, found);
        }
    }

    fn requirements(&self, expr: &TypedExpr, kinds: &mut HashMap<BindingId, ClosureKind>) {
        if let TypedExprKind::Fn(lambda) = &expr.kind {
            self.lambda_requirements(lambda, kinds);
            for child in lambda.body.expressions() {
                self.requirements(child, kinds);
            }
            return;
        }
        for child in expr.children() {
            self.requirements(child, kinds);
        }
    }

    fn lambda_requirements(
        &self,
        lambda: &TypedLambda,
        kinds: &mut HashMap<BindingId, ClosureKind>,
    ) {
        let params: Vec<BindingId> = lambda.params.iter().map(|param| param.binding).collect();
        let mut uses = ParameterUses::default();
        self.require_block(&lambda.body, &params, None, &mut uses);
        if lambda.body.repeats() {
            uses.repeat_uses_since(&ParameterUses::default());
        }
        for param in params {
            if self.written_parameter_kinds.contains_key(&param) {
                continue;
            }
            let current = kinds.entry(param).or_insert(ClosureKind::Fn);
            *current = (*current).max(uses.kind(param));
        }
    }

    fn require_block(
        &self,
        block: &TypedBlock,
        params: &[BindingId],
        nested: Option<ClosureKind>,
        uses: &mut ParameterUses,
    ) {
        let mut in_scope = Cow::Borrowed(params);
        let mut local_closures = Vec::new();
        for item in &block.items {
            let Some(binding) = LocalClosure::bound_by(item) else {
                self.require_item(item, &in_scope, nested, uses);
                continue;
            };
            let before = uses.counts();
            self.require_item(item, &in_scope, nested, uses);
            local_closures.push(LocalClosure {
                binding,
                body_uses: uses.uses_since(&before),
            });
            in_scope.to_mut().push(binding);
        }
        for closure in local_closures.into_iter().rev() {
            closure.repeat_body_if_called_again(uses);
        }
    }

    fn require_item(
        &self,
        item: &TypedBodyItem,
        params: &[BindingId],
        nested: Option<ClosureKind>,
        uses: &mut ParameterUses,
    ) {
        match item {
            TypedBodyItem::Let(_, value) | TypedBodyItem::Expr(value) => {
                self.require_expression(value, params, nested, uses)
            }
        }
    }

    fn require_expression(
        &self,
        expr: &TypedExpr,
        params: &[BindingId],
        nested: Option<ClosureKind>,
        uses: &mut ParameterUses,
    ) {
        match &expr.kind {
            TypedExprKind::Fn(lambda) => self.require_lambda(lambda, lambda.kind, params, uses),
            TypedExprKind::Call(callee, arguments) => {
                self.require_call(callee, arguments, params, nested, uses)
            }
            TypedExprKind::Control(control) => self.require_control(control, params, nested, uses),
            TypedExprKind::Leaf(_) => {}
            other => self.require_nested(other, params, nested, uses),
        }
    }

    fn require_lambda(
        &self,
        lambda: &TypedLambda,
        runs_as: ClosureKind,
        params: &[BindingId],
        uses: &mut ParameterUses,
    ) {
        let before = uses.counts();
        self.require_block(&lambda.body, params, Some(runs_as), uses);
        if lambda.body.repeats() {
            uses.repeat_uses_since(&before);
        }
    }

    fn require_call(
        &self,
        callee: &TypedExpr,
        arguments: &[TypedArgument],
        params: &[BindingId],
        nested: Option<ClosureKind>,
        uses: &mut ParameterUses,
    ) {
        let calls_a_parameter = self.require_callee(callee, params, nested, uses);
        self.require_arguments(arguments, callee, params, nested, uses);
        if !calls_a_parameter {
            self.require_expression(callee, params, nested, uses);
        }
    }

    fn require_callee(
        &self,
        callee: &TypedExpr,
        params: &[BindingId],
        nested: Option<ClosureKind>,
        uses: &mut ParameterUses,
    ) -> bool {
        let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = &callee.kind else {
            return false;
        };
        let called = self.origin(*binding);
        if !params.contains(&called) {
            return false;
        }
        uses.record_call(called);
        if let Some(kind) = nested {
            uses.require(called, kind);
        }
        true
    }

    fn origin(&self, binding: BindingId) -> BindingId {
        self.alias_origins.get(&binding).copied().unwrap_or(binding)
    }

    fn require_arguments(
        &self,
        arguments: &[TypedArgument],
        callee: &TypedExpr,
        params: &[BindingId],
        nested: Option<ClosureKind>,
        uses: &mut ParameterUses,
    ) {
        for (position, argument) in arguments.iter().enumerate() {
            match self.passed_parameter(argument, params) {
                Some(passed) => {
                    uses.record_pass(passed, self.pass_kind(callee, position, passed));
                }
                None => match self.repeated_callback(callee, position, argument) {
                    Some(callback) => self.require_callback(callback, params, uses),
                    None => self.require_expression(&argument.value, params, nested, uses),
                },
            }
        }
    }

    fn passed_parameter(
        &self,
        argument: &TypedArgument,
        params: &[BindingId],
    ) -> Option<BindingId> {
        let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = &argument.value.kind else {
            return None;
        };
        let passed = self.origin(*binding);
        params.contains(&passed).then_some(passed)
    }

    fn repeated_callback<'e>(
        &self,
        callee: &TypedExpr,
        position: usize,
        argument: &'e TypedArgument,
    ) -> Option<&'e TypedLambda> {
        match &argument.value.kind {
            TypedExprKind::Fn(lambda)
                if self.expected_kind(callee, position) == ClosureKind::FnMut =>
            {
                Some(lambda)
            }
            _ => None,
        }
    }

    fn require_callback(
        &self,
        callback: &TypedLambda,
        params: &[BindingId],
        uses: &mut ParameterUses,
    ) {
        let before = uses.counts();
        let runs_as = callback.kind.min(ClosureKind::FnMut);
        self.require_lambda(callback, runs_as, params, uses);
        uses.repeat_uses_since(&before);
    }

    fn pass_kind(&self, callee: &TypedExpr, position: usize, parameter: BindingId) -> ClosureKind {
        let target = match &callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Global(function)) => self
                .function_parameters
                .get(function)
                .and_then(|parameters| parameters.get(position)),
            _ => None,
        };
        match target {
            Some(target) if self.reaches(*target, parameter) => ClosureKind::FnOnce,
            _ => self.expected_kind(callee, position),
        }
    }

    fn build_cycle_peers(&mut self) {
        let parameters: Vec<BindingId> = self.pass_edges.keys().copied().collect();
        for parameter in &parameters {
            let peers: Vec<BindingId> = parameters
                .iter()
                .copied()
                .filter(|other| {
                    self.reaches(*parameter, *other) && self.reaches(*other, *parameter)
                })
                .collect();
            if !peers.is_empty() && !self.written_parameter_kinds.contains_key(parameter) {
                self.cycle_peers.insert(*parameter, peers);
            }
        }
    }

    fn share_cycle_kinds(&self, kinds: &mut HashMap<BindingId, ClosureKind>) {
        for (parameter, peers) in &self.cycle_peers {
            let shared = peers
                .iter()
                .filter_map(|peer| kinds.get(peer))
                .copied()
                .fold(ClosureKind::FnOnce, ClosureKind::min);
            if let Some(kind) = kinds.get_mut(parameter) {
                *kind = (*kind).min(shared);
            }
        }
    }

    fn reaches(&self, from: BindingId, parameter: BindingId) -> bool {
        let Some(targets) = self.pass_edges.get(&from) else {
            return false;
        };
        let mut pending: Vec<BindingId> = targets.iter().copied().collect();
        let mut visited = HashSet::new();
        while let Some(current) = pending.pop() {
            if current == parameter {
                return true;
            }
            if !visited.insert(current) {
                continue;
            }
            if let Some(targets) = self.pass_edges.get(&current) {
                pending.extend(targets);
            }
        }
        false
    }

    fn require_control(
        &self,
        control: &TypedControl,
        params: &[BindingId],
        nested: Option<ClosureKind>,
        uses: &mut ParameterUses,
    ) {
        match control {
            TypedControl::Block(block) => self.require_block(block, params, nested, uses),
            TypedControl::If(test, then, otherwise) => {
                self.require_expression(test, params, nested, uses);
                let branches = [then.as_ref(), otherwise.as_ref()].map(|branch| {
                    let mut branch_uses = ParameterUses::default();
                    self.require_expression(branch, params, nested, &mut branch_uses);
                    branch_uses
                });
                let joined = ParameterUses::joined(branches.into());
                uses.extend(joined);
            }
            TypedControl::Match(matched) => {
                self.require_expression(&matched.value, params, nested, uses);
                let arms = matched.arms.iter().map(|arm| {
                    let mut arm_uses = ParameterUses::default();
                    self.require_block(&arm.body, params, nested, &mut arm_uses);
                    arm_uses
                });
                uses.extend(ParameterUses::joined(arms.collect()));
            }
        }
    }

    fn require_nested(
        &self,
        kind: &TypedExprKind,
        params: &[BindingId],
        nested: Option<ClosureKind>,
        uses: &mut ParameterUses,
    ) {
        match kind {
            TypedExprKind::Recur(arguments) => self.require_recur(arguments, params, nested, uses),
            TypedExprKind::Try { value, .. }
            | TypedExprKind::Borrow(_, value)
            | TypedExprKind::Deref(value)
            | TypedExprKind::TraitObject(value, _) => {
                self.require_expression(value, params, nested, uses)
            }
            TypedExprKind::Struct(_) | TypedExprKind::Variant(..) | TypedExprKind::Assoc(..) => {
                self.require_value(kind, params, nested, uses)
            }
            _ => unreachable!("the caller matches a nested expression"),
        }
    }

    fn require_recur(
        &self,
        arguments: &[TypedArgument],
        params: &[BindingId],
        nested: Option<ClosureKind>,
        uses: &mut ParameterUses,
    ) {
        for argument in arguments {
            let passes_parameter = matches!(
                &argument.value.kind,
                TypedExprKind::Leaf(TypedLeaf::Local(binding)) if params.contains(binding)
            );
            if !passes_parameter {
                self.require_expression(&argument.value, params, nested, uses);
            }
        }
    }

    fn require_value(
        &self,
        kind: &TypedExprKind,
        params: &[BindingId],
        nested: Option<ClosureKind>,
        uses: &mut ParameterUses,
    ) {
        match kind {
            TypedExprKind::Struct(structure) => {
                self.require_structure(structure, params, nested, uses)
            }
            TypedExprKind::Variant(_, arguments) => {
                for argument in arguments {
                    self.require_expression(argument, params, nested, uses);
                }
            }
            TypedExprKind::Assoc(value, keys, item) => {
                self.require_expression(value, params, nested, uses);
                for key in keys {
                    self.require_expression(key, params, nested, uses);
                }
                self.require_expression(item, params, nested, uses);
            }
            _ => unreachable!("the caller matches a value form"),
        }
    }

    fn require_structure(
        &self,
        structure: &TypedStructExpr,
        params: &[BindingId],
        nested: Option<ClosureKind>,
        uses: &mut ParameterUses,
    ) {
        let values: Vec<&TypedExpr> = match structure {
            TypedStructExpr::New(_, fields) => fields.iter().map(|(_, value)| value).collect(),
            TypedStructExpr::Update(_, base, fields) => std::iter::once(base.as_ref())
                .chain(fields.iter().map(|(_, value)| value))
                .collect(),
            TypedStructExpr::Field(value, _) => vec![value],
        };
        for value in values {
            self.require_expression(value, params, nested, uses);
        }
    }

    fn expected_kind(&self, callee: &TypedExpr, position: usize) -> ClosureKind {
        match &callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => self
                .value_parameters
                .get(binding)
                .and_then(|params| params.get(position))
                .map(|param| self.kind_of(*param))
                .unwrap_or(ClosureKind::Fn),
            TypedExprKind::Leaf(TypedLeaf::Global(binding)) => {
                if let Some(params) = self.function_parameters.get(binding) {
                    return params
                        .get(position)
                        .map(|param| self.kind_of(*param))
                        .unwrap_or(ClosureKind::Fn);
                }
                self.rust_parameter_kinds
                    .get(binding)
                    .and_then(|kinds| kinds.get(position))
                    .copied()
                    .unwrap_or(ClosureKind::Fn)
            }
            TypedExprKind::Fn(lambda) => lambda
                .params
                .get(position)
                .map(|param| self.kind_of(param.binding))
                .unwrap_or(ClosureKind::Fn),
            _ => ClosureKind::Fn,
        }
    }

    fn check_calls(&self, modules: &[TypedModule]) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for definition in modules.iter().flat_map(|module| &module.definitions) {
            self.check_expression(&definition.value, &mut diagnostics);
        }
        diagnostics
    }

    fn check_expression(&self, expr: &TypedExpr, diagnostics: &mut Vec<Diagnostic>) {
        match &expr.kind {
            TypedExprKind::Call(callee, arguments) => {
                self.check_call(callee, arguments, diagnostics);
                self.check_expression(callee, diagnostics);
                for argument in arguments {
                    self.check_expression(&argument.value, diagnostics);
                }
            }
            TypedExprKind::Fn(lambda) => {
                for child in lambda.body.expressions() {
                    self.check_expression(child, diagnostics);
                }
            }
            _ => {
                for child in expr.children() {
                    self.check_expression(child, diagnostics);
                }
            }
        }
    }

    fn check_call(
        &self,
        callee: &TypedExpr,
        arguments: &[TypedArgument],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for (position, argument) in arguments.iter().enumerate() {
            if !matches!(argument.value.ty, Type::Function(..)) {
                continue;
            }
            let required = self.expected_kind(callee, position);
            let actual = self.value_kind(&argument.value);
            if !actual.meets(required) {
                self.report_mismatch(callee, argument, actual, required, diagnostics);
            }
        }
    }

    fn report_mismatch(
        &self,
        callee: &TypedExpr,
        argument: &TypedArgument,
        actual: ClosureKind,
        required: ClosureKind,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let capture = self.value_capture(&argument.value);
        let callee_name = self.callee_name(callee);
        let called = capture.and_then(|binding| {
            self.called_capture(&argument.value, binding)
                .map(|(kind, span)| (binding, kind, span))
        });
        let diagnostic = match called {
            Some((binding, kind, span)) => OwnershipProblems::closure_kind_mismatch_call(
                actual,
                required,
                self.bindings.get(binding).name(),
                kind,
                callee_name.as_deref(),
                argument.value.span,
                span,
            ),
            None => OwnershipProblems::closure_kind_mismatch(
                actual,
                required,
                capture.map(|binding| self.bindings.get(binding).name()),
                callee_name.as_deref(),
                argument.value.span,
                capture.and_then(|binding| self.capture_span(&argument.value, binding)),
            ),
        };
        diagnostics.push(diagnostic);
    }

    fn called_capture(&self, value: &TypedExpr, binding: BindingId) -> Option<(ClosureKind, Span)> {
        let TypedExprKind::Fn(lambda) = &value.kind else {
            return None;
        };
        let kind = self.kind_of(binding);
        if kind == ClosureKind::Fn {
            return None;
        }
        Self::called_span(&lambda.body, binding).map(|span| (kind, span))
    }

    fn called_span(block: &TypedBlock, binding: BindingId) -> Option<Span> {
        block
            .expressions()
            .find_map(|expr| Self::expression_called_span(expr, binding))
    }

    fn expression_called_span(expr: &TypedExpr, binding: BindingId) -> Option<Span> {
        if let TypedExprKind::Call(callee, _) = &expr.kind
            && let TypedExprKind::Leaf(TypedLeaf::Local(called)) = &callee.kind
            && *called == binding
        {
            return Some(callee.span);
        }
        expr.children()
            .into_iter()
            .find_map(|child| Self::expression_called_span(child, binding))
    }

    fn callee_name(&self, callee: &TypedExpr) -> Option<String> {
        match &callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Global(binding))
            | TypedExprKind::Leaf(TypedLeaf::Local(binding)) => {
                Some(self.bindings.get(*binding).name().to_string())
            }
            _ => None,
        }
    }

    fn capture_span(&self, value: &TypedExpr, _binding: BindingId) -> Option<Span> {
        let TypedExprKind::Fn(lambda) = &value.kind else {
            return None;
        };
        if lambda.kind == ClosureKind::FnMut {
            return lambda.changed_capture.map(|(_, span)| span);
        }
        lambda.moved_capture.map(|(_, span)| span)
    }

    fn called_mutable_bindings(&self, modules: &[TypedModule]) -> HashSet<BindingId> {
        let mut bindings = HashSet::new();
        for definition in modules.iter().flat_map(|module| &module.definitions) {
            self.collect_called(&definition.value, &mut bindings);
        }
        bindings
    }

    fn collect_called(&self, expr: &TypedExpr, bindings: &mut HashSet<BindingId>) {
        if let TypedExprKind::Call(callee, arguments) = &expr.kind {
            if let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = &callee.kind
                && self.kind_of(*binding) == ClosureKind::FnMut
            {
                bindings.insert(*binding);
            }
            self.collect_called(callee, bindings);
            for argument in arguments {
                self.collect_called(&argument.value, bindings);
            }
            return;
        }
        if let TypedExprKind::Fn(lambda) = &expr.kind {
            for child in lambda.body.expressions() {
                self.collect_called(child, bindings);
            }
            return;
        }
        for child in expr.children() {
            self.collect_called(child, bindings);
        }
    }

    fn patch_types(&self, modules: &mut [TypedModule]) {
        for definition in modules
            .iter_mut()
            .flat_map(|module| module.definitions.iter_mut())
        {
            let lambda = match &definition.value.kind {
                TypedExprKind::Fn(lambda) => Rc::clone(lambda),
                _ => continue,
            };
            let returned = self.block_value_kind(&lambda.body);
            self.patch_expression_types(&mut definition.value, !definition.written_result);
            if let Type::Function(_, parameters, result) = &mut definition.scheme.body {
                Self::patch_function(
                    parameters,
                    result,
                    &lambda.params,
                    &self.parameter_kinds,
                    returned,
                    !definition.written_result,
                );
            }
        }
    }

    fn patch_expression_types(&self, expr: &mut TypedExpr, patch_result: bool) {
        if let TypedExprKind::Fn(lambda) = &mut expr.kind {
            let lambda = Rc::make_mut(lambda);
            for param in &mut lambda.params {
                Self::patch_parameter(param.binding, &mut param.ty, &self.parameter_kinds);
            }
            let returned = self.block_value_kind(&lambda.body);
            if let Type::Function(_, parameters, result) = &mut expr.ty {
                Self::patch_function(
                    parameters,
                    result,
                    &lambda.params,
                    &self.parameter_kinds,
                    returned,
                    patch_result,
                );
            }
            for child in lambda.body.expressions_mut() {
                self.patch_expression_types(child, true);
            }
            return;
        }
        for child in expr.children_mut() {
            self.patch_expression_types(child, true);
        }
    }

    fn patch_function(
        parameters: &mut [Type],
        result: &mut Type,
        binders: &[TypedBinder],
        parameter_kinds: &HashMap<BindingId, ClosureKind>,
        returned: ClosureKind,
        patch_result: bool,
    ) {
        for (parameter, binder) in parameters.iter_mut().zip(binders) {
            Self::patch_parameter(binder.binding, parameter, parameter_kinds);
        }
        if !patch_result {
            return;
        }
        if let Type::Function(kind, _, _) = result {
            *kind = returned;
        }
    }

    fn patch_parameter(
        binding: BindingId,
        ty: &mut Type,
        parameter_kinds: &HashMap<BindingId, ClosureKind>,
    ) {
        if let Type::Function(kind, _, _) = ty {
            *kind = parameter_kinds
                .get(&binding)
                .copied()
                .unwrap_or(ClosureKind::Fn);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{CaptureReport, ClosureKinds, KindSources};
    use crate::core::BindingId;
    use crate::ownership::lambda_address::LambdaAddress;
    use crate::ownership::lambda_source::LambdaSource;

    struct KindsFixture {
        source: LambdaSource,
    }

    impl KindsFixture {
        const NESTED_CLOSURES: &str = "
            (defn keep (model) model)
            (defn main ()
              (let model (to-string \"Leaf\"))
              (let make (to-string \"Nissan\"))
              (let announce (fn ()
                (let shown (fn () (keep model)))
                (shown)))
              (announce))";

        fn new() -> Self {
            Self {
                source: LambdaSource::new(Self::NESTED_CLOSURES),
            }
        }

        fn kinds(&self) -> ClosureKinds<'_> {
            let program = self.source.program();
            ClosureKinds::new(KindSources {
                traits: &program.traits,
                generics: &program.generics,
                structs: &program.structs,
                enums: &program.enums,
                bindings: &program.bindings,
                template_borrows: &program.template_borrows,
                function_parameters: ClosureKinds::function_parameters(program),
                rust_parameter_kinds: ClosureKinds::rust_parameter_kinds(program),
                written_parameter_kinds: ClosureKinds::written_parameter_kinds(program),
                borrowing_lambdas: &program.borrowing_lambdas,
            })
        }

        fn analyze(&self, kinds: &mut ClosureKinds, name: &str) -> CaptureReport {
            kinds.analyze_lambda(self.source.lambda(name))
        }

        fn has_reusable_report(&self, kinds: &ClosureKinds, name: &str) -> bool {
            let address = LambdaAddress::of(self.source.lambda(name));
            kinds.reusable_reports.contains_key(&address)
        }

        fn store_reusable_report_moving(&self, kinds: &mut ClosureKinds, name: &str, moved: &str) {
            let moved = self.source.binder(moved);
            let report = CaptureReport {
                moved: BTreeMap::from([(moved.binding, moved.span)]),
                changed: BTreeMap::new(),
            };
            let address = LambdaAddress::of(self.source.lambda(name));
            kinds.reusable_reports.insert(address, report);
        }

        fn binding(&self, name: &str) -> BindingId {
            self.source.binder(name).binding
        }
    }

    #[test]
    fn a_lambda_that_binds_no_function_has_a_reusable_report() {
        let fixture = KindsFixture::new();
        let mut kinds = fixture.kinds();
        fixture.analyze(&mut kinds, "shown");
        assert!(fixture.has_reusable_report(&kinds, "shown"));
    }

    #[test]
    fn a_lambda_that_binds_a_function_has_no_reusable_report() {
        let fixture = KindsFixture::new();
        let mut kinds = fixture.kinds();
        fixture.analyze(&mut kinds, "announce");
        assert!(!fixture.has_reusable_report(&kinds, "announce"));
    }

    #[test]
    fn an_enclosing_lambda_reuses_the_report_of_a_lambda_inside_it() {
        let fixture = KindsFixture::new();
        let mut kinds = fixture.kinds();
        fixture.store_reusable_report_moving(&mut kinds, "shown", "make");
        let report = fixture.analyze(&mut kinds, "announce");
        let moved: Vec<BindingId> = report.moved.into_keys().collect();
        assert_eq!(moved, [fixture.binding("make")]);
    }
}
