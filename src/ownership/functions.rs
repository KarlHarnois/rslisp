use std::collections::HashMap;
use std::rc::Rc;

use crate::core::{BindingId, ReferenceKind};
use crate::types::{
    CopyRules, Type, TypedBinder, TypedExpr, TypedExprKind, TypedLambda, TypedProgram,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParamMode {
    Copy,
    Ref,
    Mut,
    Own,
}

impl ParamMode {
    pub fn accepts(self, kind: ReferenceKind) -> bool {
        matches!(
            (kind, self),
            (ReferenceKind::Shared, ParamMode::Ref) | (ReferenceKind::Mutable, ParamMode::Mut)
        )
    }
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
    definitions: HashMap<BindingId, FunctionInfo>,
    local_closures: HashMap<BindingId, FunctionInfo>,
}

impl FunctionTable {
    pub fn collect(program: &TypedProgram) -> Self {
        let copy = CopyRules::new(
            &program.traits,
            &program.generics,
            &program.structs,
            &program.enums,
        );
        let definitions = || {
            program
                .modules
                .iter()
                .flat_map(|module| &module.definitions)
        };
        let top_level = definitions().filter_map(|definition| {
            Self::lambda(&definition.value)
                .map(|lambda| (definition.binding, Self::info(lambda, &copy)))
        });
        let local_closures = definitions()
            .flat_map(|definition| definition.value.let_bindings())
            .filter_map(|(binder, value)| {
                Self::lambda(value).map(|lambda| (binder.binding, Self::closure_info(lambda)))
            });
        Self {
            definitions: top_level.collect(),
            local_closures: local_closures.collect(),
        }
    }

    pub fn get(&self, binding: BindingId) -> Option<&FunctionInfo> {
        self.definitions
            .get(&binding)
            .or_else(|| self.local_closures.get(&binding))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&BindingId, &FunctionInfo)> {
        self.definitions.iter().chain(&self.local_closures)
    }

    pub fn definitions(&self) -> impl Iterator<Item = &FunctionInfo> {
        self.definitions.values()
    }

    pub fn definition_bindings(&self) -> impl Iterator<Item = BindingId> + '_ {
        self.definitions.keys().copied()
    }

    pub fn sorted_definition_bindings(&self) -> Vec<BindingId> {
        let mut bindings: Vec<BindingId> = self.definition_bindings().collect();
        bindings.sort_unstable();
        bindings
    }

    pub fn definition(&self, binding: BindingId) -> Option<&FunctionInfo> {
        self.definitions.get(&binding)
    }

    pub fn fixed_mode(slot: &ParamSlot, copy: &CopyRules) -> ParamMode {
        match &slot.ty {
            Type::Mut(_, _) => ParamMode::Mut,
            borrowed if borrowed.is_unsized() => ParamMode::Ref,
            written if copy.is_small_copy(written) => ParamMode::Copy,
            _ => ParamMode::Own,
        }
    }

    fn lambda(value: &TypedExpr) -> Option<&Rc<TypedLambda>> {
        match &value.kind {
            TypedExprKind::Fn(lambda) => Some(lambda),
            _ => None,
        }
    }

    fn closure_info(lambda: &Rc<TypedLambda>) -> FunctionInfo {
        Self::function_info(lambda, |_| false)
    }

    fn info(lambda: &Rc<TypedLambda>, copy: &CopyRules) -> FunctionInfo {
        Self::function_info(lambda, |param| {
            !param.annotated
                && !matches!(param.ty, Type::Function(..) | Type::Mut(_, _))
                && (!copy.is_small_copy(&param.ty) || lambda.body.writes_local(param.binding))
        })
    }

    fn function_info(
        lambda: &Rc<TypedLambda>,
        inferable: impl Fn(&TypedBinder) -> bool,
    ) -> FunctionInfo {
        let params = lambda
            .params
            .iter()
            .map(|param| ParamSlot {
                binding: param.binding,
                ty: param.ty.clone(),
                inferable: inferable(param),
            })
            .collect();
        FunctionInfo {
            lambda: Rc::clone(lambda),
            params,
        }
    }
}
