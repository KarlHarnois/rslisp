use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::moved_values::{FieldPath, MoveSite, MovedValues};
use super::mutation::ChangedValues;
use super::problems::OwnershipProblems;
use super::repeats::{NextPass, RepeatingBody};
use super::stored_definitions::StoredDefinitions;
use super::update_bases::UpdateBases;
use super::update_order::StructUpdateOrder;
use crate::core::{BindingArena, BindingId, FieldLabel, FieldSpelling};
use crate::diagnostic::Diagnostic;
use crate::source::Span;
use crate::types::{
    Borrow, ClosureKind, CopyRules, FieldPlace, GenericId, GenericParam, InferredCopies, Pipelines,
    StructTable, TemplateBorrows, Type, TypedArgument, TypedBlock, TypedBodyItem, TypedControl,
    TypedExpr, TypedExprKind, TypedLambda, TypedLeaf, TypedMatch, TypedStructExpr,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Usage {
    Move,
    Borrow,
}

struct MovedUse<'u> {
    place: &'u FieldPlace<'u>,
    span: Span,
    site: &'u MoveSite,
}

#[derive(Default)]
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
    stored_definitions: &'a StoredDefinitions,
    changed: ChangedValues,
    moved: MovedValues,
    repeating: Option<RepeatingBody>,
    moved_by_earlier_pass: MovedValues,
    copy_functions: HashSet<BindingId>,
    once_functions: HashSet<BindingId>,
    moving_closure_holders: HashSet<BindingId>,
    moving_closure_uses: usize,
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
        stored_definitions: &'a StoredDefinitions,
    ) -> Self {
        Self {
            copy,
            structs,
            bindings,
            template_borrows,
            generics,
            pipelines,
            stored_definitions,
            changed: ChangedValues::default(),
            moved: MovedValues::default(),
            repeating: None,
            moved_by_earlier_pass: MovedValues::default(),
            copy_functions: HashSet::new(),
            once_functions: HashSet::new(),
            moving_closure_holders: HashSet::new(),
            moving_closure_uses: 0,
            reported: HashSet::new(),
            parameters: HashSet::new(),
            unwritten_parameters: HashSet::new(),
            inferred_copies: InferredCopies::default(),
            diagnostics: Vec::new(),
        }
    }

    pub fn check(mut self, enclosing: BindingId, lambda: &TypedLambda) -> MoveReport {
        self.parameters = lambda.params.iter().map(|param| param.binding).collect();
        self.unwritten_parameters = lambda
            .params
            .iter()
            .filter(|param| !param.annotated)
            .map(|param| param.binding)
            .collect();
        self.changed = ChangedValues::of(lambda, enclosing);
        self.block(&lambda.body, Usage::Move);
        self.report()
    }

    pub fn check_value(mut self, value: &TypedExpr) -> MoveReport {
        self.expr(value, Usage::Move);
        self.report()
    }

    fn report(self) -> MoveReport {
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
            other => self.nested_expr(expr, other, usage),
        }
    }

    fn nested_expr(&mut self, expr: &TypedExpr, kind: &TypedExprKind, usage: Usage) {
        match kind {
            TypedExprKind::Recur(arguments) => self.recur(arguments),
            TypedExprKind::Struct(structure) => self.structure(expr, structure, usage),
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
            TypedExprKind::Fn(lambda) => self.closure(lambda),
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
        match leaf {
            TypedLeaf::Local(binding) => self.local(*binding, &expr.ty, expr.span, usage),
            TypedLeaf::Global(binding) if usage == Usage::Move => {
                self.stored_definition(*binding, expr)
            }
            _ => {}
        }
    }

    fn stored_definition(&mut self, binding: BindingId, expr: &TypedExpr) {
        let moves_stored_value = self.stored_definitions.contains(binding)
            && !self.copy.is_copy(&expr.ty)
            && !matches!(expr.ty, Type::Function(..));
        if !moves_stored_value {
            return;
        }
        let name = self.bindings.get(binding).name();
        self.diagnostics
            .push(OwnershipProblems::moved_definition(name, expr.span));
    }

    fn written(&mut self, expr: &TypedExpr) {
        match &expr.kind {
            TypedExprKind::Borrow(_, value) => self.expr(value, Usage::Borrow),
            TypedExprKind::Deref(referent) => self.deref(expr, referent),
            TypedExprKind::Try { value, .. } => self.expr(value, Usage::Move),
            _ => unreachable!("the caller matches a written form"),
        }
    }

    fn deref(&mut self, deref: &TypedExpr, referent: &TypedExpr) {
        let ((), uses_moving_closure) =
            self.tracking_moving_closures(|checker| checker.expr(referent, Usage::Borrow));
        if uses_moving_closure && deref.ty.holds_function() {
            let copy_trait = self.copy.copy_trait_name(self.bindings);
            self.diagnostics
                .push(OwnershipProblems::closure_copied_out_of_reference(
                    copy_trait, deref.span,
                ));
        }
    }

    fn tracking_moving_closures<R>(&mut self, visit: impl FnOnce(&mut Self) -> R) -> (R, bool) {
        let earlier_uses = self.moving_closure_uses;
        let visited = visit(self);
        (visited, self.moving_closure_uses > earlier_uses)
    }

    fn structure(&mut self, expr: &TypedExpr, structure: &TypedStructExpr, usage: Usage) {
        match structure {
            TypedStructExpr::Field(owner, _) => self.field_read(expr, owner, usage),
            TypedStructExpr::New(_, fields) => self.field_values(fields),
            TypedStructExpr::Update(_, base, fields) => self.update(expr, base, fields),
        }
    }

    fn update(&mut self, update: &TypedExpr, base: &TypedExpr, fields: &[(FieldLabel, TypedExpr)]) {
        match StructUpdateOrder::of(base) {
            StructUpdateOrder::FieldsBeforePlace(place) => {
                self.field_values(fields);
                let bases = UpdateBases::new(self.structs, &self.copy);
                let unlisted = bases.unlisted_fields(update, fields);
                let moved = bases.moved_fields(&unlisted);
                self.unlisted_read(&place, Self::root_span(base), &unlisted);
                self.move_fields(&place, base.span, moved);
            }
            StructUpdateOrder::BaseFirst => {
                self.update_base(base);
                self.field_values(fields);
            }
        }
    }

    fn field_values(&mut self, fields: &[(FieldLabel, TypedExpr)]) {
        fields
            .iter()
            .for_each(|(_, value)| self.expr(value, Usage::Move));
    }

    fn field_read(&mut self, read: &TypedExpr, owner: &TypedExpr, usage: Usage) {
        match read.field_place() {
            Some(place) => {
                self.place_read(&place, Self::root_span(read));
                self.closure_field_read(&place, read, usage);
            }
            None => self.expr(owner, Usage::Borrow),
        }
    }

    fn closure_field_read(&mut self, place: &FieldPlace, read: &TypedExpr, usage: Usage) {
        if !self.reads_moving_closure(place, &read.ty) {
            return;
        }
        self.moving_closure_uses += 1;
        if usage == Usage::Move {
            self.record_field_move(place, read.span);
        }
    }

    fn reads_moving_closure(&self, place: &FieldPlace, read_type: &Type) -> bool {
        self.moving_closure_holders.contains(&place.root)
            && read_type.holds_function()
            && !read_type.is_reference()
    }

    fn place_read(&mut self, place: &FieldPlace, span: Span) {
        if let Some(site) = self
            .moved
            .site_overlapping(place.root, Self::path_names(place))
        {
            self.use_after_move(place, span, &site);
        }
    }

    fn unlisted_read(&mut self, place: &FieldPlace, span: Span, unlisted: &[(Rc<str>, Type)]) {
        let site = unlisted.iter().find_map(|(field, _)| {
            let field_path = Self::path_names(place).chain(std::iter::once(&**field));
            self.moved.site_overlapping(place.root, field_path)
        });
        if let Some(site) = site {
            self.use_after_move(place, span, &site);
        }
    }

    fn path_names<'p>(place: &'p FieldPlace) -> impl Iterator<Item = &'p str> + Clone {
        place.path.iter().map(|label| &*label.name)
    }

    fn root_span(read: &TypedExpr) -> Span {
        match &read.kind {
            TypedExprKind::Struct(TypedStructExpr::Field(owner, _)) => Self::root_span(owner),
            _ => read.span,
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

    fn move_field(&mut self, place: &FieldPlace, span: Span) {
        if !self.copy.is_copy(place.root_type) {
            self.record_field_move(place, span);
        }
    }

    fn record_field_move(&mut self, place: &FieldPlace, span: Span) {
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
                self.expr(&matched.value, self.scrutinee_usage(matched));
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

    fn scrutinee_usage(&self, matched: &TypedMatch) -> Usage {
        let binds_owned_part = matched
            .arms
            .iter()
            .flat_map(|arm| arm.pattern.binders())
            .any(|binder| !self.copies_out_of(&matched.value, &binder.ty));
        if binds_owned_part {
            Usage::Move
        } else {
            Usage::Borrow
        }
    }

    fn copies_out_of(&self, scrutinee: &TypedExpr, part: &Type) -> bool {
        let copies_closure = !part.holds_function() || !self.is_moving_closure_holder(scrutinee);
        copies_closure && self.copy.is_copy(part)
    }

    fn is_moving_closure_holder(&self, value: &TypedExpr) -> bool {
        matches!(&value.kind, TypedExprKind::Leaf(TypedLeaf::Local(binding))
            if self.moving_closure_holders.contains(binding))
    }

    fn closure(&mut self, lambda: &TypedLambda) {
        if let Some(next) = self.pass(lambda).next_pass(lambda) {
            self.repeat(lambda, next);
        }
    }

    fn pass(&mut self, lambda: &TypedLambda) -> RepeatingBody {
        let enclosing = self.repeating.replace(RepeatingBody::enter(&self.moved));
        self.block(&lambda.body, Usage::Move);
        std::mem::replace(&mut self.repeating, enclosing).expect("the pass entered a body")
    }

    fn repeat(&mut self, lambda: &TypedLambda, next: NextPass) {
        self.moved_by_earlier_pass.merge(next.carried().clone());
        let after_first_pass = std::mem::replace(&mut self.moved, next.moved_at_start());
        self.pass(lambda);
        self.moved.merge(after_first_pass);
    }

    fn bind(&mut self, binding: BindingId, value: &TypedExpr) {
        let (kind, uses_moving_closure) =
            self.tracking_moving_closures(|checker| checker.bound_function_kind(value));
        self.record_function_kind(binding, kind);
        if uses_moving_closure && Self::wraps_function(&value.ty) {
            self.moving_closure_holders.insert(binding);
        }
    }

    fn record_function_kind(&mut self, binding: BindingId, kind: Option<ClosureKind>) {
        match kind {
            Some(ClosureKind::Fn) => self.copy_functions.insert(binding),
            Some(ClosureKind::FnOnce) => self.once_functions.insert(binding),
            _ => false,
        };
    }

    fn wraps_function(ty: &Type) -> bool {
        ty.holds_function() && !matches!(ty, Type::Function(..))
    }

    fn bound_function_kind(&mut self, value: &TypedExpr) -> Option<ClosureKind> {
        match &value.kind {
            TypedExprKind::Fn(lambda) => {
                self.closure(lambda);
                Some(lambda.kind)
            }
            TypedExprKind::Leaf(TypedLeaf::Global(_)) | TypedExprKind::Leaf(TypedLeaf::Rust(_)) => {
                matches!(value.ty, Type::Function(..)).then_some(ClosureKind::Fn)
            }
            TypedExprKind::Leaf(TypedLeaf::Local(other)) => {
                let kind = self.local_function_kind(*other);
                self.expr(value, Usage::Move);
                kind
            }
            _ => {
                self.expr(value, Usage::Move);
                None
            }
        }
    }

    fn local_function_kind(&self, binding: BindingId) -> Option<ClosureKind> {
        if self.copy_functions.contains(&binding) {
            Some(ClosureKind::Fn)
        } else if self.once_functions.contains(&binding) {
            Some(ClosureKind::FnOnce)
        } else {
            None
        }
    }

    fn call(&mut self, callee: &TypedExpr, arguments: &[TypedArgument]) {
        if !matches!(
            callee.kind,
            TypedExprKind::Leaf(TypedLeaf::Global(_)) | TypedExprKind::Leaf(TypedLeaf::Rust(_))
        ) {
            let usage = self.callee_usage(callee);
            self.expr(callee, usage);
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

    fn callee_usage(&self, callee: &TypedExpr) -> Usage {
        match &callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding))
                if self.once_functions.contains(binding) =>
            {
                Usage::Move
            }
            TypedExprKind::Struct(TypedStructExpr::Field(..)) => Usage::Move,
            _ => Usage::Borrow,
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
            Some(site) => self.use_after_move(&Self::whole_place(binding, ty), span, &site),
            None => self.record_use(binding, ty, span, usage),
        }
    }

    fn whole_place(root: BindingId, root_type: &Type) -> FieldPlace<'_> {
        FieldPlace {
            root,
            root_type,
            path: Vec::new(),
        }
    }

    fn use_after_move(&mut self, place: &FieldPlace, span: Span, site: &MoveSite) {
        let binding = place.root;
        if self.records_inferred_copy(binding, place.root_type) || !self.reported.insert(binding) {
            return;
        }
        let moved_use = MovedUse { place, span, site };
        let diagnostic = self.use_after_move_diagnostic(&moved_use);
        self.diagnostics.push(diagnostic);
    }

    fn use_after_move_diagnostic(&self, moved_use: &MovedUse) -> Diagnostic {
        let moved = self.moved_value(moved_use);
        if let Some(diagnostic) = self.bound_help(moved_use, &moved) {
            return diagnostic;
        }
        let place = moved_use.place;
        self.ownership_help(place.root, place.root_type, &moved)
            .unwrap_or_else(|| self.reuse_help(moved, moved_use))
    }

    fn reuse_help(&self, moved: Diagnostic, moved_use: &MovedUse) -> Diagnostic {
        let name = self.bindings.get(moved_use.place.root).name();
        if self.pipelines.is_pipeline(moved_use.place.root_type) {
            return OwnershipProblems::pipeline_help(moved, name);
        }
        let site = moved_use.site;
        OwnershipProblems::clone_suggestion(moved, site.moved_place(name), site.span())
    }

    fn moved_value(&self, moved_use: &MovedUse) -> Diagnostic {
        let site = moved_use.site;
        let moved_place = site.moved_place(self.bindings.get(moved_use.place.root).name());
        if self.moved_in_earlier_pass(moved_use) {
            OwnershipProblems::use_after_earlier_pass_move(moved_place, moved_use.span, site.span())
        } else {
            OwnershipProblems::use_after_move(moved_place, moved_use.span, site.span())
        }
    }

    fn moved_in_earlier_pass(&self, moved_use: &MovedUse) -> bool {
        let place = moved_use.place;
        self.moved_by_earlier_pass
            .site_overlapping(place.root, Self::path_names(place))
            .is_some_and(|earlier| earlier.span() == moved_use.site.span())
    }

    fn bound_help(&self, moved_use: &MovedUse, moved: &Diagnostic) -> Option<Diagnostic> {
        let place = moved_use.place;
        if matches!(place.root_type, Type::Function(..)) {
            return Some(OwnershipProblems::closure_move_help(moved.clone()));
        }
        let Type::Generic(generic) = place.root_type else {
            return None;
        };
        if !self.parameters.contains(&place.root) {
            return None;
        }
        let parameter = self.generics.get(generic)?;
        Some(OwnershipProblems::missing_copy_bound(
            self.bindings.get(place.root).name(),
            &parameter.name,
            self.copy.copy_trait_name(self.bindings),
            moved_use.span,
            moved_use.site.span(),
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
        if self.holds_moving_closure(binding, ty) {
            self.moving_closure_uses += 1;
        }
        if usage == Usage::Move && !self.is_copy_binding(binding, ty) {
            self.moved.move_whole(binding, span);
        }
    }

    fn is_copy_binding(&self, binding: BindingId, ty: &Type) -> bool {
        let copies_closure = !self.holds_moving_closure(binding, ty) || matches!(ty, Type::Ref(..));
        copies_closure && self.copy.is_copy(ty)
    }

    fn holds_moving_closure(&self, binding: BindingId, ty: &Type) -> bool {
        let is_moving_closure =
            matches!(ty, Type::Function(..)) && !self.copy_functions.contains(&binding);
        is_moving_closure || self.moving_closure_holders.contains(&binding)
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
