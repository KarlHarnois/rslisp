use super::checker::TypeChecker;
use super::problems::{MismatchContext, ProjectionNames, TypeProblems};
use super::projection::AssociatedType;
use super::traits::Implementor;
use super::ty::{GenericId, Type};
use super::unifier::DeferredEquality;
use crate::core::{BindingId, CoreModule, ItemKind, RustTraitDeclaration, RustTraitImplementor};
use crate::diagnostic::Diagnostic;
use crate::source::Span;

pub(super) struct LoweredAssociatedValue {
    pub projection: BindingId,
    pub value: Type,
    pub span: Span,
}

pub(super) struct WrittenProjection {
    projection: BindingId,
    owner: Type,
    span: Span,
}

impl TypeChecker {
    const PROJECTION_ARITY: usize = 1;

    pub(super) fn record_projections(&mut self, modules: &[CoreModule]) {
        for declaration in Self::rust_traits(modules) {
            for associated in &declaration.associated {
                self.type_arities
                    .insert(associated.binding, Self::PROJECTION_ARITY);
                self.traits.projections_mut().declare(
                    associated.binding,
                    AssociatedType {
                        trait_binding: declaration.binding,
                        parameter: associated.parameter.clone(),
                        rust_name: associated.rust_name.clone(),
                    },
                );
            }
        }
    }

    fn rust_traits(modules: &[CoreModule]) -> impl Iterator<Item = &RustTraitDeclaration> {
        modules
            .iter()
            .flat_map(|module| &module.items)
            .filter_map(|item| match &item.kind {
                ItemKind::RustTrait(declaration) => Some(declaration),
                _ => None,
            })
    }

    pub(super) fn lower_rust_trait_implementors(
        &mut self,
        declaration: &RustTraitDeclaration,
    ) -> Result<Vec<Implementor>, Diagnostic> {
        let trait_name = self.bindings.get(declaration.binding).name().to_string();
        let declared = self.traits.projections().declared_by(declaration.binding);
        declaration
            .implementors
            .iter()
            .map(|entry| {
                let (implementor, values) = self.lower_implementor(entry, &trait_name)?;
                self.check_associated_values(declaration.binding, &declared, entry, &values)?;
                for value in values {
                    self.traits.projections_mut().add_rule(
                        value.projection,
                        &implementor,
                        value.value,
                    );
                }
                Ok(implementor)
            })
            .collect()
    }

    fn check_associated_values(
        &self,
        trait_binding: BindingId,
        declared: &[BindingId],
        entry: &RustTraitImplementor,
        values: &[LoweredAssociatedValue],
    ) -> Result<(), Diagnostic> {
        let trait_name = self.bindings.get(trait_binding).name();
        if let Some(undeclared) = values
            .iter()
            .find(|value| !declared.contains(&value.projection))
        {
            return Err(TypeProblems::undeclared_associated_value(
                trait_name,
                self.bindings.get(undeclared.projection).name(),
                undeclared.span,
            ));
        }
        match declared
            .iter()
            .find(|projection| !values.iter().any(|value| value.projection == **projection))
        {
            Some(missing) => Err(TypeProblems::missing_associated_value(
                trait_name,
                self.bindings.get(*missing).name(),
                entry.implementor.span(),
            )),
            None => Ok(()),
        }
    }

    pub(super) fn record_written_projection(&mut self, lowered: &Type, span: Span) {
        if let Type::Named(binding, arguments) = lowered
            && self.traits.projections().is_projection(*binding)
        {
            self.written_projections.push(WrittenProjection {
                projection: *binding,
                owner: arguments[0].clone(),
                span,
            });
        }
    }

    pub(super) fn share_projections(&mut self) {
        let projections = self.traits.share_projections();
        self.unifier.adopt_projections(projections);
    }

    pub(super) fn check_written_projections(&mut self) {
        let unchecked = self.checked_projections..self.written_projections.len();
        self.checked_projections = self.written_projections.len();
        for index in unchecked {
            if let Err(diagnostic) = self.check_written_projection(&self.written_projections[index])
            {
                self.diagnostics.push(diagnostic);
            }
        }
    }

    pub(super) fn has_written_projection_on(
        &self,
        generic: GenericId,
        trait_binding: BindingId,
    ) -> bool {
        self.written_projections.iter().any(|written| {
            written.owner == Type::Generic(generic)
                && self.projection_trait(written.projection) == trait_binding
        })
    }

    fn projection_trait(&self, projection: BindingId) -> BindingId {
        self.traits
            .projections()
            .associated(projection)
            .expect("a written projection names a declared associated type")
            .trait_binding
    }

    fn check_written_projection(&self, written: &WrittenProjection) -> Result<(), Diagnostic> {
        let projections = self.traits.projections();
        let trait_binding = self.projection_trait(written.projection);
        let names = ProjectionNames {
            projection: self.bindings.get(written.projection).name(),
            trait_name: self.bindings.get(trait_binding).name(),
            owner: &self.display().show(&written.owner),
        };
        match &written.owner {
            Type::Generic(generic) if !self.generic_implements(*generic, trait_binding) => {
                Err(TypeProblems::projection_without_bound(&names, written.span))
            }
            _ if written.owner.is_unknown()
                || projections
                    .normalize_projection(written.projection, &written.owner)
                    .is_some() =>
            {
                Ok(())
            }
            _ => Err(TypeProblems::projection_without_implementor(
                &names,
                written.span,
            )),
        }
    }

    fn generic_implements(&self, generic: GenericId, trait_binding: BindingId) -> bool {
        self.unifier
            .generic_bounds(generic)
            .iter()
            .any(|bound| bound.binding == trait_binding)
    }

    pub(super) fn recheck_deferred_equalities(&mut self) {
        for deferred in self.unifier.take_deferred() {
            if let Err(diagnostic) = self.recheck_deferred_equality(&deferred) {
                self.diagnostics.push(diagnostic);
            }
        }
    }

    fn recheck_deferred_equality(&mut self, deferred: &DeferredEquality) -> Result<(), Diagnostic> {
        self.unifier
            .unify(&deferred.other, &deferred.projected, deferred.span)
            .map_err(|_| {
                TypeProblems::mismatch(
                    &self.describe(&self.unifier.resolve(&deferred.other)),
                    &self.describe(&self.unifier.resolve(&deferred.projected)),
                    deferred.span,
                    MismatchContext::Plain,
                )
            })
    }
}
