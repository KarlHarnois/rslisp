use rustc_hash::FxHashSet;

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

    pub fn check(&self, method: &ShownMethod, flows: &FxHashSet<BindingId>) -> Option<Diagnostic> {
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

#[cfg(test)]
mod tests {
    use super::{ShownMethod, UntiedBorrows};
    use crate::core::BindingId;
    use crate::types::{TypedDefinition, TypedProgram, TypedSource, TypedTraitImpl};

    struct ReportFixture {
        program: TypedProgram,
    }

    impl ReportFixture {
        const BORROWING_GARAGE: &str = "
            (defstruct car :model str :year i64)
            (trait garage
              (year-of (fn ((ref self)) i64))
              (model-of (fn ((ref self)) (ref str)))
              (newer (fn (self car) self))
              (badge (fn (i64 self) (ref str))))
            (impl garage car
              (year-of (c) (:year c))
              (model-of (c) (:model c))
              (newer (c other) (if (> (:year c) (:year other)) c other))
              (badge (year c) (if (> year (:year c)) \"old\" \"new\")))
            (defn main ())";

        fn new() -> Self {
            Self {
                program: TypedSource::expand(Self::BORROWING_GARAGE).typed(),
            }
        }

        fn may_report(&self, name: &str, static_result: bool) -> bool {
            let implementation = self.implementation();
            let declared = self.declaration(implementation.trait_binding, name);
            let implemented = self.implemented(implementation, name);
            let shown = ShownMethod {
                binding: implemented.binding,
                lambda: implemented
                    .value
                    .lambda_value()
                    .expect("an impl method is a function"),
                parameters: declared
                    .scheme
                    .body
                    .parameters()
                    .expect("a trait method has a function type"),
                result: declared
                    .scheme
                    .body
                    .parts()
                    .last()
                    .copied()
                    .expect("a trait method has a result"),
                static_result,
            };
            let self_generic = declared.scheme.params[0].id;
            UntiedBorrows::new(&self.program, &implementation.implementor, self_generic)
                .may_report(&shown)
        }

        fn implementation(&self) -> &TypedTraitImpl {
            &self.program.trait_impls[0]
        }

        fn declaration(&self, trait_binding: BindingId, name: &str) -> &TypedDefinition {
            let declared = self
                .program
                .declared_trait_method(trait_binding, name)
                .expect("the trait declares the method");
            self.definition(declared.declaration())
        }

        fn implemented(&self, implementation: &TypedTraitImpl, name: &str) -> &TypedDefinition {
            let method = implementation
                .methods
                .iter()
                .find(|method| self.program.bindings.get(method.binding).name() == name)
                .expect("the impl writes the method");
            self.definition(method.binding)
        }

        fn definition(&self, binding: BindingId) -> &TypedDefinition {
            self.program
                .modules
                .iter()
                .flat_map(|module| &module.definitions)
                .find(|definition| definition.binding == binding)
                .expect("the binding has a definition")
        }
    }

    #[test]
    fn a_result_without_borrow_or_self_cannot_report() {
        let fixture = ReportFixture::new();
        assert!(!fixture.may_report("year-of", false));
    }

    #[test]
    fn a_borrowed_result_that_is_not_static_cannot_report() {
        let fixture = ReportFixture::new();
        assert!(!fixture.may_report("model-of", false));
    }

    #[test]
    fn a_self_result_can_report() {
        let fixture = ReportFixture::new();
        assert!(fixture.may_report("newer", false));
    }

    #[test]
    fn a_static_borrowed_result_can_report() {
        let fixture = ReportFixture::new();
        assert!(fixture.may_report("badge", true));
    }
}
