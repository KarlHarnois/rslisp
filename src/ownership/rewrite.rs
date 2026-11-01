use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::functions::{FunctionTable, ParamMode};
use crate::core::{BindingId, ReferenceKind};
use crate::types::{
    Borrow, CopyRules, Lifetime, Type, TypedArgument, TypedBodyItem, TypedControl, TypedDefinition,
    TypedExpr, TypedExprKind, TypedLeaf, TypedProgram,
};

pub struct ModeRewrite<'a> {
    param_types: FxHashMap<BindingId, Type>,
    binding_types: FxHashMap<BindingId, Type>,
    added_references: FxHashSet<BindingId>,
    copy_binders: FxHashSet<BindingId>,
    function_params: FxHashMap<BindingId, Vec<Type>>,
    modes: &'a FxHashMap<BindingId, ParamMode>,
}

impl<'a> ModeRewrite<'a> {
    pub fn new(
        functions: &FunctionTable,
        modes: &'a FxHashMap<BindingId, ParamMode>,
        slices: &'a FxHashMap<BindingId, Type>,
        copy: &CopyRules,
    ) -> Self {
        let mut param_types = FxHashMap::default();
        let mut function_params = FxHashMap::default();
        for (function, info) in functions.iter() {
            let mut types = Vec::new();
            for slot in &info.params {
                let rewritten = slices.get(&slot.binding).cloned().unwrap_or_else(|| {
                    match (
                        slot.inferable || slot.ty.is_unsized(),
                        modes.get(&slot.binding),
                    ) {
                        (true, Some(ParamMode::Ref)) => Type::shared(slot.ty.clone()),
                        (true, Some(ParamMode::Mut)) => Type::mutable(slot.ty.clone()),
                        _ => slot.ty.clone(),
                    }
                });
                if rewritten != slot.ty {
                    param_types.insert(slot.binding, rewritten.clone());
                }
                types.push(rewritten);
            }
            function_params.insert(*function, types);
        }
        Self {
            param_types,
            binding_types: FxHashMap::default(),
            added_references: FxHashSet::default(),
            copy_binders: Self::copy_binders(functions, copy),
            function_params,
            modes,
        }
    }

    fn copy_binders(functions: &FunctionTable, copy: &CopyRules) -> FxHashSet<BindingId> {
        let mut binders = FxHashSet::default();
        for function in functions.definitions() {
            for expr in function.lambda.body.expressions() {
                Self::collect_copy_binders(expr, copy, &mut binders);
            }
        }
        binders
    }

    fn collect_copy_binders(
        expr: &TypedExpr,
        copy: &CopyRules,
        binders: &mut FxHashSet<BindingId>,
    ) {
        if let TypedExprKind::Control(TypedControl::Match(matched)) = &expr.kind {
            for arm in &matched.arms {
                for binder in arm.pattern.binders() {
                    if copy.is_copy(&binder.ty) {
                        binders.insert(binder.binding);
                    }
                }
            }
        }
        for child in expr.children() {
            Self::collect_copy_binders(child, copy, binders);
        }
    }

    pub fn function_params(&self) -> &FxHashMap<BindingId, Vec<Type>> {
        &self.function_params
    }

    pub fn apply(&mut self, program: &mut TypedProgram) {
        for definition in program
            .modules
            .iter_mut()
            .flat_map(|module| module.definitions.iter_mut())
        {
            self.definition(definition);
        }
    }

    fn definition(&mut self, definition: &mut TypedDefinition) {
        let params = self.function_params.get(&definition.binding).cloned();
        if let Some(params) = &params {
            Self::replace_parameters(&mut definition.value.ty, params);
            Self::replace_parameters(&mut definition.scheme.body, params);
        }
        self.rewrite_value(definition, params.as_deref());
        Self::carry_definition_lifetimes(definition);
    }

    fn rewrite_value(&mut self, definition: &mut TypedDefinition, params: Option<&[Type]>) {
        match &mut definition.value.kind {
            TypedExprKind::Fn(_) => {
                self.rewrite_params(&mut definition.value);
                for child in definition.value.children_mut() {
                    self.expr(child, params);
                }
            }
            _ => self.expr(&mut definition.value, None),
        }
    }

    fn carry_definition_lifetimes(definition: &mut TypedDefinition) {
        Self::carry_function_lifetimes(&definition.scheme.body, &mut definition.value.ty);
        let TypedExprKind::Fn(lambda) = &mut definition.value.kind else {
            return;
        };
        let Type::Function(_, parameters, _) = &definition.scheme.body else {
            return;
        };
        let lambda = Rc::make_mut(lambda);
        for (param, written) in lambda.params.iter_mut().zip(parameters) {
            Self::carry_type_lifetimes(written, &mut param.ty);
        }
    }

    fn replace_parameters(function: &mut Type, params: &[Type]) {
        if let Type::Function(_, parameters, _) = function {
            let mut carried = params.to_vec();
            for (written, replacement) in parameters.iter().zip(&mut carried) {
                Self::carry_type_lifetimes(written, replacement);
            }
            *parameters = carried;
        }
    }

    fn carry_function_lifetimes(written: &Type, value: &mut Type) {
        if let (
            Type::Function(_, written_parameters, written_result),
            Type::Function(_, parameters, result),
        ) = (written, value)
        {
            for (written, value) in written_parameters.iter().zip(parameters.iter_mut()) {
                Self::carry_type_lifetimes(written, value);
            }
            Self::carry_type_lifetimes(written_result, result);
        }
    }

    fn carry_type_lifetimes(written: &Type, value: &mut Type) {
        match (written, value) {
            (Type::Ref(written_lifetime, written), Type::Ref(lifetime, value))
            | (Type::Mut(written_lifetime, written), Type::Mut(lifetime, value)) => {
                if matches!(written_lifetime, Lifetime::Written(_)) {
                    *lifetime = *written_lifetime;
                }
                Self::carry_type_lifetimes(written, value);
            }
            (
                Type::Function(_, written_parameters, written_result),
                Type::Function(_, parameters, result),
            ) => {
                for (written, value) in written_parameters.iter().zip(parameters.iter_mut()) {
                    Self::carry_type_lifetimes(written, value);
                }
                Self::carry_type_lifetimes(written_result, result);
            }
            (Type::Named(written_binding, written), Type::Named(binding, value))
                if written_binding == binding =>
            {
                for (written, value) in written.iter().zip(value.iter_mut()) {
                    Self::carry_type_lifetimes(written, value);
                }
            }
            _ => {}
        }
    }

    fn expr(&mut self, expr: &mut TypedExpr, current: Option<&[Type]>) {
        self.rewrite_local(expr);
        self.rewrite_params(expr);
        self.prepare_match(expr);
        let child_current = match expr.kind {
            TypedExprKind::Fn(_) => None,
            _ => current,
        };
        for child in expr.children_mut() {
            self.expr(child, child_current);
        }
        self.rewrite_written(expr);
        match &mut expr.kind {
            TypedExprKind::Call(callee, arguments) => self.borrow_arguments(callee, arguments),
            TypedExprKind::Recur(arguments) => self.borrow_recur(arguments, current),
            _ => {}
        }
    }

    fn prepare_match(&mut self, expr: &mut TypedExpr) {
        let TypedExprKind::Control(TypedControl::Match(matched)) = &mut expr.kind else {
            return;
        };
        let original = matched.value.ty.clone();
        self.rewrite_local(&mut matched.value);
        if original.is_reference() || !matched.value.ty.is_reference() {
            return;
        }
        let copies = self.wrap_copy_binders(matched);
        Self::copy_arm_references(matched, &copies);
    }

    fn wrap_copy_binders(
        &mut self,
        matched: &mut crate::types::TypedMatch,
    ) -> FxHashMap<BindingId, Type> {
        let mut copies = FxHashMap::default();
        for arm in &mut matched.arms {
            for binder in arm.pattern.binders_mut() {
                if self.copy_binders.contains(&binder.binding) {
                    copies.insert(binder.binding, binder.ty.clone());
                }
                binder.ty = Type::shared(binder.ty.clone());
                self.binding_types.insert(binder.binding, binder.ty.clone());
                self.added_references.insert(binder.binding);
            }
        }
        copies
    }

    fn copy_arm_references(
        matched: &mut crate::types::TypedMatch,
        copies: &FxHashMap<BindingId, Type>,
    ) {
        for arm in &mut matched.arms {
            for item in &mut arm.body.items {
                match item {
                    TypedBodyItem::Let(_, value) => Self::copy_added_references(value, copies),
                    TypedBodyItem::Expr(expr) => Self::copy_added_references(expr, copies),
                }
            }
        }
    }

    fn copy_added_references(expr: &mut TypedExpr, copies: &FxHashMap<BindingId, Type>) {
        if let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = &expr.kind
            && let Some(owned) = copies.get(binding)
        {
            let leaf = TypedExpr {
                kind: TypedExprKind::Leaf(TypedLeaf::Local(*binding)),
                ty: expr.ty.clone(),
                span: expr.span,
            };
            *expr = TypedExpr {
                kind: TypedExprKind::Deref(Box::new(leaf)),
                ty: owned.clone(),
                span: expr.span,
            };
            return;
        }
        for child in expr.children_mut() {
            Self::copy_added_references(child, copies);
        }
    }

    fn rewrite_written(&self, expr: &mut TypedExpr) {
        let rewritten = match &expr.kind {
            TypedExprKind::Borrow(kind, value) => {
                let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = &value.kind else {
                    return;
                };
                let matching = matches!(
                    (kind, self.modes.get(binding), &value.ty),
                    (ReferenceKind::Shared, Some(ParamMode::Ref), Type::Ref(_, _))
                        | (
                            ReferenceKind::Mutable,
                            Some(ParamMode::Mut),
                            Type::Mut(_, _)
                        )
                );
                matching.then(|| value.ty.clone())
            }
            _ => None,
        };
        if let Some(ty) = rewritten {
            expr.ty = ty;
        }
    }

    fn rewrite_local(&self, expr: &mut TypedExpr) {
        if let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = &expr.kind
            && let Some(rewritten) = self
                .param_types
                .get(binding)
                .or_else(|| self.binding_types.get(binding))
        {
            expr.ty = rewritten.clone();
        }
    }

    fn rewrite_params(&self, expr: &mut TypedExpr) {
        let TypedExprKind::Fn(lambda) = &mut expr.kind else {
            return;
        };
        for param in &mut Rc::make_mut(lambda).params {
            if let Some(rewritten) = self.param_types.get(&param.binding) {
                param.ty = rewritten.clone();
            }
        }
    }

    fn borrow_arguments(&mut self, callee: &TypedExpr, arguments: &mut [TypedArgument]) {
        let TypedExprKind::Leaf(TypedLeaf::Global(function)) = callee.kind else {
            return;
        };
        let params =
            self.function_params
                .get(&function)
                .cloned()
                .unwrap_or_else(|| match &callee.ty {
                    Type::Function(_, parameters, _) => parameters.clone(),
                    _ => Vec::new(),
                });
        for (argument, param) in arguments.iter_mut().zip(&params) {
            self.borrow_for(argument, param);
        }
    }

    fn borrow_recur(&mut self, arguments: &mut [TypedArgument], params: Option<&[Type]>) {
        let Some(params) = params else {
            return;
        };
        for (argument, param) in arguments.iter_mut().zip(params) {
            self.borrow_for(argument, param);
        }
    }

    fn borrow_for(&mut self, argument: &mut TypedArgument, param: &Type) {
        if self.borrow_added_reference(argument, param) {
            return;
        }
        if argument.borrow == Borrow::None
            && matches!(argument.value.ty, Type::Mut(_, _))
            && matches!(param, Type::Mut(_, _))
        {
            argument.borrow = Borrow::Reborrow;
            return;
        }
        if Self::already_borrowed(argument) {
            return;
        }
        argument.borrow = Self::borrow_mode(param);
    }

    fn borrow_added_reference(&self, argument: &mut TypedArgument, param: &Type) -> bool {
        let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = argument.value.kind else {
            return false;
        };
        if !self.added_references.contains(&binding) {
            return false;
        }
        if !param.is_reference() && argument.borrow == Borrow::None {
            argument.borrow = Borrow::Deref(1);
        }
        true
    }

    fn already_borrowed(argument: &TypedArgument) -> bool {
        matches!(argument.borrow, Borrow::Passthrough | Borrow::Deref(_))
            || argument.value.ty.is_reference()
    }

    fn borrow_mode(param: &Type) -> Borrow {
        match param {
            Type::Ref(_, _) => Borrow::Shared,
            Type::Mut(_, _) => Borrow::Mutable,
            _ => Borrow::None,
        }
    }
}
