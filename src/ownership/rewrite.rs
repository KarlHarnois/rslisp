use std::collections::HashMap;
use std::rc::Rc;

use super::functions::{FunctionTable, ParamMode};
use crate::core::BindingId;
use crate::types::{
    Borrow, Type, TypedArgument, TypedBlock, TypedBodyItem, TypedDefinition, TypedExpr,
    TypedExprKind, TypedProgram,
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
        if let Some(params) = self.function_params.get(&definition.binding) {
            Self::replace_parameters(&mut definition.value.ty, params);
            Self::replace_parameters(&mut definition.scheme.body, params);
        }
        self.expr(&mut definition.value);
    }

    fn replace_parameters(function: &mut Type, params: &[Type]) {
        if let Type::Function(parameters, _) = function {
            *parameters = params.to_vec();
        }
    }

    fn expr(&self, expr: &mut TypedExpr) {
        match &mut expr.kind {
            TypedExprKind::Local(binding) => {
                if let Some(rewritten) = self.param_types.get(binding) {
                    expr.ty = rewritten.clone();
                }
            }
            TypedExprKind::Call(callee, arguments) => self.call(callee, arguments),
            TypedExprKind::Fn(lambda) => {
                let lambda = Rc::make_mut(lambda);
                for param in &mut lambda.params {
                    if let Some(rewritten) = self.param_types.get(&param.binding) {
                        param.ty = rewritten.clone();
                    }
                }
                self.block(&mut lambda.body);
            }
            TypedExprKind::Block(block) => self.block(block),
            TypedExprKind::If(test, then, otherwise) => [test, then, otherwise]
                .into_iter()
                .for_each(|part| self.expr(part)),
            TypedExprKind::New(_, fields) => {
                fields.iter_mut().for_each(|(_, value)| self.expr(value))
            }
            TypedExprKind::Field(value, _) => self.expr(value),
            TypedExprKind::Literal(_) | TypedExprKind::Global(_) | TypedExprKind::Rust(_) => {}
        }
    }

    fn call(&self, callee: &mut TypedExpr, arguments: &mut [TypedArgument]) {
        self.expr(callee);
        arguments
            .iter_mut()
            .for_each(|argument| self.expr(&mut argument.value));
        let TypedExprKind::Global(function) = callee.kind else {
            return;
        };
        let Some(params) = self.function_params.get(&function) else {
            return;
        };
        for (argument, param) in arguments.iter_mut().zip(params) {
            Self::borrow_for(argument, param);
        }
    }

    fn borrow_for(argument: &mut TypedArgument, param: &Type) {
        if matches!(argument.borrow, Borrow::Passthrough | Borrow::Deref)
            || argument.value.ty.is_reference()
        {
            return;
        }
        argument.borrow = match param {
            Type::Ref(_) => Borrow::Shared,
            Type::Mut(_) => Borrow::Mutable,
            _ => Borrow::None,
        };
    }

    fn block(&self, block: &mut TypedBlock) {
        for item in &mut block.items {
            match item {
                TypedBodyItem::Let(_, value) | TypedBodyItem::Expr(value) => self.expr(value),
            }
        }
    }
}
