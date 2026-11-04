use std::collections::BTreeSet;

use rustc_hash::FxHashMap;

use super::checker::TypeChecker;
use super::selection::Selection;
use super::ty::{Bound, Type};
use crate::core::{BindingId, CoreModule, ItemKind, RustTraitDeclaration, StructuralClause};
use crate::diagnostic::Diagnostic;

#[derive(Clone)]
pub struct StructuralTrait {
    pub binding: BindingId,
    pub method: BindingId,
    pub parameters: Vec<Type>,
    pub result: Type,
    pub clause: StructuralClause,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StructuralUse {
    pub trait_binding: BindingId,
    pub head: BindingId,
}

#[derive(Clone)]
struct ImplBoundUse {
    definitions: Vec<BindingId>,
    bound: Bound,
    implementor: Type,
}

#[derive(Default)]
pub struct StructuralUses {
    by_definition: FxHashMap<BindingId, BTreeSet<StructuralUse>>,
    through_impls: Vec<ImplBoundUse>,
}

impl StructuralUses {
    pub fn of_definitions<'u>(
        &'u self,
        definitions: impl IntoIterator<Item = &'u BindingId>,
    ) -> BTreeSet<StructuralUse> {
        definitions
            .into_iter()
            .filter_map(|definition| self.by_definition.get(definition))
            .flatten()
            .copied()
            .collect()
    }

    fn record(&mut self, definitions: &[BindingId], used: StructuralUse) {
        for definition in definitions {
            self.by_definition
                .entry(*definition)
                .or_default()
                .insert(used);
        }
    }
}

impl TypeChecker {
    const RECEIVER: &'static str = "self";

    pub(super) fn record_structural_traits(&mut self, modules: &[CoreModule]) {
        for item in modules.iter().flat_map(|module| &module.items) {
            let ItemKind::RustTrait(declaration) = &item.kind else {
                continue;
            };
            if let Err(diagnostic) = self.record_structural_trait(declaration) {
                self.diagnostics.push(diagnostic);
            }
        }
    }

    fn record_structural_trait(
        &mut self,
        declaration: &RustTraitDeclaration,
    ) -> Result<(), Diagnostic> {
        let (Some(clause), Some(method)) = (&declaration.structural, declaration.methods.first())
        else {
            return Ok(());
        };
        let receiver = self.new_generic(Self::RECEIVER, Vec::new());
        let scope = FxHashMap::from_iter([(declaration.self_variable, receiver.id)]);
        let Type::Function(_, parameters, result) = self.lower_type(&method.signature, &scope)?
        else {
            return Ok(());
        };
        self.structural_traits.push(StructuralTrait {
            binding: declaration.binding,
            method: method.binding,
            parameters: parameters.into_iter().skip(1).collect(),
            result: *result,
            clause: clause.clone(),
        });
        Ok(())
    }

    pub(super) fn is_structural_trait(&self, trait_binding: BindingId) -> bool {
        self.structural_traits
            .iter()
            .any(|structural| structural.binding == trait_binding)
    }

    pub(super) fn record_structural_use(&mut self, bound: &Bound, implementor: &Type) {
        let definitions = self.recursive_group.clone();
        if self.has_generic_program_impl(bound.binding) {
            self.structural_uses.through_impls.push(ImplBoundUse {
                definitions: definitions.clone(),
                bound: bound.clone(),
                implementor: implementor.clone(),
            });
        }
        self.record_use_in(&definitions, bound.binding, implementor);
    }

    fn has_generic_program_impl(&self, trait_binding: BindingId) -> bool {
        self.trait_impls.iter().any(|implementation| {
            implementation.trait_binding == trait_binding && !implementation.params.is_empty()
        })
    }

    pub(super) fn record_uses_through_impls(&mut self) {
        for position in 0..self.structural_uses.through_impls.len() {
            let pending = self.structural_uses.through_impls[position].clone();
            self.record_requirements(&pending);
        }
    }

    fn record_requirements(&mut self, pending: &ImplBoundUse) {
        let implementor = self.unifier.resolve(&pending.implementor);
        let Selection::Implemented { requirements, .. } =
            self.traits.select(&pending.bound, &implementor)
        else {
            return;
        };
        for requirement in requirements {
            let required = self.unifier.resolve(&requirement.implementor);
            self.record_use_in(&pending.definitions, requirement.bound.binding, &required);
            self.record_requirements(&ImplBoundUse {
                definitions: pending.definitions.clone(),
                bound: requirement.bound,
                implementor: required,
            });
        }
    }

    fn record_use_in(
        &mut self,
        definitions: &[BindingId],
        trait_binding: BindingId,
        implementor: &Type,
    ) {
        let Type::Named(head, _) = implementor else {
            return;
        };
        if !self.is_structural_trait(trait_binding) {
            return;
        }
        let used = StructuralUse {
            trait_binding,
            head: *head,
        };
        self.structural_uses.record(definitions, used);
    }
}

#[cfg(test)]
mod tests {
    use crate::types::TypedSource;

    struct UsesFixture {
        source: TypedSource,
    }

    impl UsesFixture {
        const SOURCE: &str = r#"
            (rust-type out "std::fmt::Formatter<'_>")
            (rust-type written "std::fmt::Result")
            (rust-trait octal "std::fmt::Octal"
              (method write-octal (fn ((ref self) (mut out)) written))
              (structural :struct "" :field "" :variant "" :part "" :bare "")
              u32)
            (def show-octal (rust "{0}" (where ((a octal)) (fn ((ref a)) unit))))
            (def copied (rust "{0}.clone()" (where ((a clone)) (fn ((ref a)) a))))
            (defstruct mode :owner u32)
            (defn main () (show-octal (->mode 1)) (copied (->mode 2)))"#;

        fn new() -> Self {
            Self {
                source: TypedSource::expand(Self::SOURCE),
            }
        }

        fn recorded_traits(&self) -> Vec<String> {
            let typed = self.source.typed();
            let definitions: Vec<_> = self
                .source
                .program()
                .modules
                .iter()
                .flat_map(|module| module.definitions().map(|(binding, _)| binding))
                .collect();
            typed
                .structural_uses
                .of_definitions(&definitions)
                .into_iter()
                .map(|used| typed.bindings.get(used.trait_binding).name().to_string())
                .collect()
        }
    }

    #[test]
    fn only_a_trait_with_a_generic_program_impl_waits_for_its_bounds() {
        let source = TypedSource::expand(
            r#"
            (trait describe (describe-it (fn ((ref self)) string)))
            (trait label (label-it (fn ((ref self)) string)))
            (impl describe (vec a) (describe-it (v) (pr-str v)))
            (impl label i64 (label-it (n) (str n)))
            (defn main () (println (describe-it (vec 1)) (label-it 2)))"#,
        );
        assert_eq!(source.typed().structural_uses.through_impls.len(), 1);
    }

    #[test]
    fn only_a_structural_trait_records_its_uses() {
        assert_eq!(UsesFixture::new().recorded_traits(), vec!["octal"]);
    }
}
