use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::moved_values::{FieldPath, MoveSite, MovedValues};
use super::mutation::ChangedValues;
use super::problems::OwnershipProblems;
use super::repeats::{NextPass, RepeatingBody};
use crate::core::{BindingArena, BindingId, FieldLabel, FieldSpelling};
use crate::diagnostic::Diagnostic;
use crate::source::Span;
use crate::types::{
    Borrow, ClosureKind, CopyRules, FieldPlace, GenericId, GenericParam, InferredCopies, Pipelines,
    StructTable, TemplateBorrows, Type, TypedArgument, TypedBlock, TypedBodyItem, TypedControl,
    TypedExpr, TypedExprKind, TypedLambda, TypedLeaf, TypedStructExpr,
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
    structs: &'a StructTable,
    bindings: &'a BindingArena,
    template_borrows: &'a TemplateBorrows,
    generics: &'a HashMap<GenericId, GenericParam>,
    pipelines: Pipelines<'a>,
    enclosing: BindingId,
    changed: ChangedValues,
    moved: MovedValues,
    repeating: Option<RepeatingBody>,
    moved_by_earlier_pass: HashSet<BindingId>,
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
        structs: &'a StructTable,
        bindings: &'a BindingArena,
        template_borrows: &'a TemplateBorrows,
        generics: &'a HashMap<GenericId, GenericParam>,
        pipelines: Pipelines<'a>,
        enclosing: BindingId,
    ) -> Self {
        Self {
            copy,
            structs,
            bindings,
            template_borrows,
            generics,
            pipelines,
            enclosing,
            changed: ChangedValues::default(),
            moved: MovedValues::default(),
            repeating: None,
            moved_by_earlier_pass: HashSet::new(),
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
            TypedExprKind::Struct(structure) => self.structure(expr, structure),
            TypedExprKind::Variant(_, arguments) => self.expressions(arguments),
            TypedExprKind::Assoc(value, keys, item) => self.assoc(value, keys, item),
            TypedExprKind::TraitObject(value, _) => self.expr(value, Usage::Move),
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
        self.update_base(value);
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

    fn structure(&mut self, expr: &TypedExpr, structure: &TypedStructExpr) {
        match structure {
            TypedStructExpr::Field(owner, _) => self.field_read(expr, owner),
            TypedStructExpr::New(_, fields) => self.field_values(fields),
            TypedStructExpr::Update(_, base, fields) => self.update(expr, base, fields),
        }
    }

    fn update(&mut self, update: &TypedExpr, base: &TypedExpr, fields: &[(FieldLabel, TypedExpr)]) {
        if let Some(place) = base.field_place() {
            self.field_values(fields);
            self.refill(base, fields);
            self.expr(base, Usage::Borrow);
            let unlisted = self.unlisted_moving_fields(update, fields);
            self.move_fields(&place, base.span, unlisted);
        } else {
            self.update_base(base);
            self.field_values(fields);
        }
    }

    fn field_values(&mut self, fields: &[(FieldLabel, TypedExpr)]) {
        fields
            .iter()
            .for_each(|(_, value)| self.expr(value, Usage::Move));
    }

    fn field_read(&mut self, read: &TypedExpr, owner: &TypedExpr) {
        match read.field_place() {
            Some(place) => self.place_read(&place, Self::root_span(read)),
            None => self.expr(owner, Usage::Borrow),
        }
    }

    fn place_read(&mut self, place: &FieldPlace, span: Span) {
        let path = place.path.iter().map(|label| &*label.name);
        if let Some(site) = self.moved.site_overlapping(place.root, path) {
            self.use_after_move(place.root, place.root_type, span, &site);
        }
    }

    fn root_span(read: &TypedExpr) -> Span {
        match &read.kind {
            TypedExprKind::Struct(TypedStructExpr::Field(owner, _)) => Self::root_span(owner),
            _ => read.span,
        }
    }

    fn refill(&mut self, base: &TypedExpr, fields: &[(FieldLabel, TypedExpr)]) {
        if let Some(place) = base.field_place() {
            let names = fields.iter().map(|(label, _)| &*label.name);
            self.moved
                .refill(place.root, &Self::field_path(&place), names);
        }
    }

    fn update_base(&mut self, base: &TypedExpr) {
        match base.field_place() {
            Some(place) if place.reads_field() && !self.copy.is_copy(&base.ty) => {
                self.expr(base, Usage::Borrow);
                self.move_field(&place, base.span);
            }
            _ => self.expr(base, Usage::Move),
        }
    }

    fn move_fields(&mut self, place: &FieldPlace, span: Span, moving: Vec<Rc<str>>) {
        let site = self.field_site(place, span);
        for field in moving {
            let mut path = Self::field_path(place);
            path.push(field);
            self.moved.move_field(place.root, path, site.clone());
        }
    }

    fn unlisted_moving_fields(
        &self,
        update: &TypedExpr,
        fields: &[(FieldLabel, TypedExpr)],
    ) -> Vec<Rc<str>> {
        let Type::Named(structure, arguments) = &update.ty else {
            unreachable!("a struct update has its struct's type")
        };
        let definition = self
            .structs
            .get(*structure)
            .expect("the type checker resolved the updated struct");
        definition
            .instantiated_fields(arguments)
            .into_iter()
            .filter(|(name, _)| !fields.iter().any(|(label, _)| label.name == *name))
            .filter(|(_, field_type)| !self.copy.is_copy(field_type))
            .map(|(name, _)| name)
            .collect()
    }

    fn move_field(&mut self, place: &FieldPlace, span: Span) {
        if self.copy.is_copy(place.root_type) {
            return;
        }
        let site = self.field_site(place, span);
        self.moved
            .move_field(place.root, Self::field_path(place), site);
    }

    fn field_site(&self, place: &FieldPlace, span: Span) -> MoveSite {
        MoveSite::Field {
            span,
            place: self.place_text(place).into(),
        }
    }

    fn field_path(place: &FieldPlace) -> FieldPath {
        place.path.iter().map(|label| label.name.clone()).collect()
    }

    fn place_text(&self, place: &FieldPlace) -> String {
        let root = self.bindings.get(place.root).name().to_string();
        place.path.iter().fold(root, Self::field_text)
    }

    fn field_text(owner: String, label: &&FieldLabel) -> String {
        let field = label.spelled(&label.name);
        match label.spelling {
            FieldSpelling::Keyword => format!("({field} {owner})"),
            FieldSpelling::Bare => format!("(field {owner} {field})"),
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
                    self.moved.merge(moved);
                }
            }
        }
    }

    fn closure(&mut self, lambda: &TypedLambda) -> bool {
        if let Some(next) = self.pass(lambda).next_pass(lambda) {
            self.repeat(lambda, next);
        }
        lambda.kind == ClosureKind::Fn
    }

    fn pass(&mut self, lambda: &TypedLambda) -> RepeatingBody {
        let enclosing = self.repeating.replace(RepeatingBody::enter(&self.moved));
        self.block(&lambda.body, Usage::Move);
        std::mem::replace(&mut self.repeating, enclosing).expect("the pass entered a body")
    }

    fn repeat(&mut self, lambda: &TypedLambda, next: NextPass) {
        self.moved_by_earlier_pass.extend(next.carried_bindings());
        let after_first_pass = std::mem::replace(&mut self.moved, next.moved_at_start());
        self.pass(lambda);
        self.moved.merge(after_first_pass);
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
        argument.borrow == Borrow::None
            && !matches!(argument.value.ty, Type::Ref(_, _))
            && !self.template_borrows.keeps(callee, position)
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
        if let Some(body) = &mut self.repeating {
            body.carry(&self.moved);
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
        self.moved.merge(after_then);
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
        match self.moved.site(binding) {
            Some(site) => self.use_after_move(binding, ty, span, &site),
            None => self.record_use(binding, ty, span, usage),
        }
    }

    fn use_after_move(&mut self, binding: BindingId, ty: &Type, span: Span, site: &MoveSite) {
        if self.records_inferred_copy(binding, ty) || !self.reported.insert(binding) {
            return;
        }
        let diagnostic = self.use_after_move_diagnostic(binding, ty, span, site);
        self.diagnostics.push(diagnostic);
    }

    fn use_after_move_diagnostic(
        &self,
        binding: BindingId,
        ty: &Type,
        span: Span,
        site: &MoveSite,
    ) -> Diagnostic {
        let name = self.bindings.get(binding).name();
        let moved_at = site.span();
        if let Some(diagnostic) = self.bound_help(binding, ty, span, moved_at) {
            return diagnostic;
        }
        let moved = self.moved_value(binding, span, moved_at);
        self.ownership_help(binding, ty, &moved)
            .unwrap_or_else(|| self.reuse_help(moved, name, ty, site))
    }

    fn reuse_help(&self, moved: Diagnostic, name: &str, ty: &Type, site: &MoveSite) -> Diagnostic {
        if self.pipelines.is_pipeline(ty) {
            return OwnershipProblems::pipeline_help(moved, name);
        }
        OwnershipProblems::clone_suggestion(moved, site.cloned_place(name), site.span())
    }

    fn moved_value(&self, binding: BindingId, span: Span, moved_at: Span) -> Diagnostic {
        let name = self.bindings.get(binding).name();
        if self.moved_by_earlier_pass.contains(&binding) {
            OwnershipProblems::use_after_earlier_pass_move(name, span, moved_at)
        } else {
            OwnershipProblems::use_after_move(name, span, moved_at)
        }
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
                self.moved_value(binding, span, moved_at),
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
            self.moved.move_whole(binding, span);
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
