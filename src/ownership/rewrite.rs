use std::collections::HashMap;
use std::rc::Rc;

use super::functions::{FunctionTable, ParamMode};
use crate::core::{BindingId, ReferenceKind};
use crate::types::{
    Borrow, Lifetime, Type, TypedArgument, TypedControl, TypedDefinition, TypedExpr, TypedExprKind,
    TypedLeaf, TypedProgram,
};

pub struct ModeRewrite<'a> {
    param_types: HashMap<BindingId, Type>,
    binding_types: HashMap<BindingId, Type>,
    added_references: std::collections::HashSet<BindingId>,
    function_params: HashMap<BindingId, Vec<Type>>,
    modes: &'a HashMap<BindingId, ParamMode>,
}

impl<'a> ModeRewrite<'a> {
    pub fn new(
        functions: &FunctionTable,
        modes: &'a HashMap<BindingId, ParamMode>,
        slices: &'a HashMap<BindingId, Type>,
    ) -> Self {
        let mut param_types = HashMap::new();
        let mut function_params = HashMap::new();
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
            binding_types: HashMap::new(),
            added_references: std::collections::HashSet::new(),
            function_params,
            modes,
        }
    }

    pub fn function_params(&self) -> &HashMap<BindingId, Vec<Type>> {
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
        match &mut definition.value.kind {
            TypedExprKind::Fn(_) => {
                self.rewrite_params(&mut definition.value);
                let current = params.as_deref();
                for child in definition.value.children_mut() {
                    self.expr(child, current);
                }
            }
            _ => self.expr(&mut definition.value, None),
        }
        Self::carry_function_lifetimes(&definition.scheme.body, &mut definition.value.ty);
        if let TypedExprKind::Fn(lambda) = &mut definition.value.kind {
            let written = definition.scheme.body.clone();
            if let Type::Function(_, parameters, _) = &written {
                let lambda = Rc::make_mut(lambda);
                for (param, written) in lambda.params.iter_mut().zip(parameters) {
                    Self::carry_lifetimes(written, &mut param.ty);
                }
            }
        }
    }

    fn replace_parameters(function: &mut Type, params: &[Type]) {
        if let Type::Function(_, parameters, _) = function {
            let mut carried = params.to_vec();
            for (written, replacement) in parameters.iter().zip(&mut carried) {
                Self::carry_lifetimes(written, replacement);
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
                Self::carry_lifetimes(written, value);
            }
            Self::carry_lifetimes(written_result, result);
        }
    }

    fn carry_lifetimes(written: &Type, value: &mut Type) {
        match (written, value) {
            (Type::Ref(written_lifetime, written), Type::Ref(lifetime, value))
            | (Type::Mut(written_lifetime, written), Type::Mut(lifetime, value)) => {
                if matches!(written_lifetime, Lifetime::Written(_)) {
                    *lifetime = *written_lifetime;
                }
                Self::carry_lifetimes(written, value);
            }
            (
                Type::Function(_, written_parameters, written_result),
                Type::Function(_, parameters, result),
            ) => {
                for (written, value) in written_parameters.iter().zip(parameters.iter_mut()) {
                    Self::carry_lifetimes(written, value);
                }
                Self::carry_lifetimes(written_result, result);
            }
            (Type::Named(written_binding, written), Type::Named(binding, value))
                if written_binding == binding =>
            {
                for (written, value) in written.iter().zip(value.iter_mut()) {
                    Self::carry_lifetimes(written, value);
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
        for arm in &mut matched.arms {
            for binder in arm.pattern.binders_mut() {
                binder.ty = Type::shared(binder.ty.clone());
                self.binding_types.insert(binder.binding, binder.ty.clone());
                self.added_references.insert(binder.binding);
            }
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
