use std::collections::HashMap;
use std::rc::Rc;

use crate::core::BindingId;
use crate::types::{CopyRules, Type, TypedExprKind, TypedLambda, TypedProgram};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParamMode {
    Copy,
    Ref,
    Mut,
    Own,
}

pub struct ParamSlot {
    pub binding: BindingId,
    pub ty: Type,
    pub inferable: bool,
}

pub struct FunctionInfo {
    pub lambda: Rc<TypedLambda>,
    pub params: Vec<ParamSlot>,
}

#[derive(Default)]
pub struct FunctionTable {
    functions: HashMap<BindingId, FunctionInfo>,
}

impl FunctionTable {
    pub fn collect(program: &TypedProgram) -> Self {
        let copy = CopyRules::new(&program.traits, &program.generics, &program.structs);
        let functions = program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .filter_map(|definition| match &definition.value.kind {
                TypedExprKind::Fn(lambda) => Some((definition.binding, Self::info(lambda, &copy))),
                _ => None,
            })
            .collect();
        Self { functions }
    }

    pub fn get(&self, binding: BindingId) -> Option<&FunctionInfo> {
        self.functions.get(&binding)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&BindingId, &FunctionInfo)> {
        self.functions.iter()
    }

    pub fn fixed_mode(slot: &ParamSlot, copy: &CopyRules) -> ParamMode {
        match &slot.ty {
            Type::Mut(_) => ParamMode::Mut,
            written if copy.is_small_copy(written) => ParamMode::Copy,
            _ => ParamMode::Own,
        }
    }

    fn info(lambda: &Rc<TypedLambda>, copy: &CopyRules) -> FunctionInfo {
        let params = lambda
            .params
            .iter()
            .map(|param| ParamSlot {
                binding: param.binding,
                ty: param.ty.clone(),
                inferable: !param.annotated
                    && !copy.is_small_copy(&param.ty)
                    && !matches!(param.ty, Type::Function(..) | Type::Mut(_)),
            })
            .collect();
        FunctionInfo {
            lambda: Rc::clone(lambda),
            params,
        }
    }
}
