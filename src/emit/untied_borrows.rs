use std::collections::HashSet;

use super::problems::{EmitProblems, UntiedNames};
use crate::core::BindingId;
use crate::diagnostic::Diagnostic;
use crate::types::{GenericId, Type, TypeDisplay, TypedBinder, TypedLambda, TypedProgram};

pub struct ShownMethod<'s> {
    pub binding: BindingId,
    pub lambda: &'s TypedLambda,
    pub parameters: &'s [Type],
    pub result: &'s Type,
    pub static_result: bool,
}

pub struct UntiedBorrows<'a> {
    program: &'a TypedProgram,
    implementor: &'a Type,
    self_generic: GenericId,
}

impl<'a> UntiedBorrows<'a> {
    pub fn new(program: &'a TypedProgram, implementor: &'a Type, self_generic: GenericId) -> Self {
        Self {
            program,
            implementor,
            self_generic,
        }
    }

    pub fn may_report(&self, method: &ShownMethod) -> bool {
        if self.has_only_implementor_borrows(method.result) {
            method.result.mentions(self.self_generic)
        } else {
            method.static_result
        }
    }

    pub fn check(&self, method: &ShownMethod, flows: &HashSet<BindingId>) -> Option<Diagnostic> {
        method
            .lambda
            .params
            .iter()
            .zip(method.parameters)
            .filter(|(param, _)| flows.contains(&param.binding))
            .find_map(|(param, shown)| self.untied(method, param, shown))
    }

    fn untied(
        &self,
        method: &ShownMethod,
        param: &TypedBinder,
        shown: &Type,
    ) -> Option<Diagnostic> {
        if self.has_only_implementor_borrows(method.result) && self.avoids_self(shown) {
            return Some(self.argument_in_result(method, param));
        }
        method.static_result.then(|| {
            EmitProblems::implementor_borrow_in_static_result(
                &self.names(method, param),
                param.span,
            )
        })
    }

    fn argument_in_result(&self, method: &ShownMethod, param: &TypedBinder) -> Diagnostic {
        let names = self.names(method, param);
        if self.is_implementor(&param.ty) {
            EmitProblems::implementor_argument_in_result(&names, param.span)
        } else {
            EmitProblems::borrowed_argument_in_result(&names, param.span)
        }
    }

    fn has_only_implementor_borrows(&self, result: &Type) -> bool {
        !self.program.type_holds_borrow(result)
    }

    fn avoids_self(&self, shown: &Type) -> bool {
        !shown.mentions(self.self_generic)
    }

    fn is_implementor(&self, ty: &Type) -> bool {
        matches!(
            (ty, self.implementor),
            (Type::Named(binding, _), Type::Named(implementor, _)) if binding == implementor
        )
    }

    fn names(&self, method: &ShownMethod, param: &TypedBinder) -> UntiedNames {
        let name = |binding: BindingId| self.program.bindings.get(binding).name().to_string();
        UntiedNames {
            method: name(method.binding),
            implementor: TypeDisplay::new(&self.program.bindings, &self.program.generics)
                .show(self.implementor),
            parameter: name(param.binding),
        }
    }
}
