use std::collections::HashMap;
use std::rc::Rc;

use super::functions::{FunctionTable, ParamMode};
use crate::core::BindingId;
use crate::types::{
    Borrow, Type, TypedArgument, TypedDefinition, TypedExpr, TypedExprKind, TypedLeaf, TypedProgram,
};

pub struct ModeRewrite {
    param_types: HashMap<BindingId, Type>,
    function_params: HashMap<BindingId, Vec<Type>>,
}

impl ModeRewrite {
    pub fn new(functions: &FunctionTable, modes: &HashMap<BindingId, ParamMode>) -> Self {
        let mut param_types = HashMap::new();
        let mut function_params = HashMap::new();
        for (function, info) in functions.iter() {
            let mut types = Vec::new();
            for slot in &info.params {
                let rewritten = match (slot.inferable, modes.get(&slot.binding)) {
                    (true, Some(ParamMode::Ref)) => Type::Ref(Box::new(slot.ty.clone())),
                    (true, Some(ParamMode::Mut)) => Type::Mut(Box::new(slot.ty.clone())),
                    _ => slot.ty.clone(),
                };
                if rewritten != slot.ty {
                    param_types.insert(slot.binding, rewritten.clone());
                }
                types.push(rewritten);
            }
            function_params.insert(*function, types);
        }
        Self {
            param_types,
            function_params,
        }
    }

    pub fn function_params(&self) -> &HashMap<BindingId, Vec<Type>> {
        &self.function_params
    }

    pub fn apply(&self, program: &mut TypedProgram) {
        for definition in program
            .modules
            .iter_mut()
            .flat_map(|module| module.definitions.iter_mut())
        {
            self.definition(definition);
        }
    }

    fn definition(&self, definition: &mut TypedDefinition) {
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
    }

    fn replace_parameters(function: &mut Type, params: &[Type]) {
        if let Type::Function(parameters, _) = function {
            *parameters = params.to_vec();
        }
    }

    fn expr(&self, expr: &mut TypedExpr, current: Option<&[Type]>) {
        self.rewrite_local(expr);
        self.rewrite_params(expr);
        let child_current = match expr.kind {
            TypedExprKind::Fn(_) => None,
            _ => current,
        };
        for child in expr.children_mut() {
            self.expr(child, child_current);
        }
        match &mut expr.kind {
            TypedExprKind::Call(callee, arguments) => self.borrow_arguments(callee, arguments),
            TypedExprKind::Recur(arguments) => self.borrow_recur(arguments, current),
            _ => {}
        }
    }

    fn rewrite_local(&self, expr: &mut TypedExpr) {
        if let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = &expr.kind
            && let Some(rewritten) = self.param_types.get(binding)
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

    fn borrow_arguments(&self, callee: &TypedExpr, arguments: &mut [TypedArgument]) {
        let TypedExprKind::Leaf(TypedLeaf::Global(function)) = callee.kind else {
            return;
        };
        let Some(params) = self.function_params.get(&function) else {
            return;
        };
        for (argument, param) in arguments.iter_mut().zip(params) {
            Self::borrow_for(argument, param);
        }
    }

    fn borrow_recur(&self, arguments: &mut [TypedArgument], params: Option<&[Type]>) {
        let Some(params) = params else {
            return;
        };
        for (argument, param) in arguments.iter_mut().zip(params) {
            Self::borrow_for(argument, param);
        }
    }

    fn borrow_for(argument: &mut TypedArgument, param: &Type) {
        if argument.borrow == Borrow::None
            && matches!(argument.value.ty, Type::Mut(_))
            && matches!(param, Type::Mut(_))
        {
            argument.borrow = Borrow::Reborrow;
            return;
        }
        if Self::already_borrowed(argument) {
            return;
        }
        argument.borrow = Self::borrow_mode(param);
    }

    fn already_borrowed(argument: &TypedArgument) -> bool {
        matches!(argument.borrow, Borrow::Passthrough | Borrow::Deref(_))
            || argument.value.ty.is_reference()
    }

    fn borrow_mode(param: &Type) -> Borrow {
        match param {
            Type::Ref(_) => Borrow::Shared,
            Type::Mut(_) => Borrow::Mutable,
            _ => Borrow::None,
        }
    }
}
