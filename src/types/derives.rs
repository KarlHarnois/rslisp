use std::collections::HashMap;
use std::rc::Rc;

use super::checker::TypeChecker;
use super::problems::TypeProblems;
use super::traits::Implementor;
use super::ty::{Bound, GenericId, GenericParam, Scheme, Type};
use crate::core::{BindingId, CoreModule, FunctionFieldTraits, ItemKind, StructDerives};
use crate::modules::ModuleSources;
use crate::source::Span;

struct WrittenDerive {
    span: Span,
    traits: Vec<(BindingId, Span)>,
}

#[derive(Clone, Copy)]
enum DeriveSource {
    Inferred,
    Listed(Span),
}

#[derive(Clone, Copy)]
enum DefinitionKind {
    Struct,
    Enum,
}

#[derive(Clone, Copy)]
struct DeriveCandidate {
    binding: BindingId,
    kind: DefinitionKind,
    trait_binding: BindingId,
    source: DeriveSource,
}

impl TypeChecker {
    pub(super) fn register_derived_traits(&mut self, modules: &[CoreModule]) {
        let types = Self::type_arities(modules);
        let derivable = Self::derivable_traits(modules);
        let written = self.written_derives(modules, &derivable);
        let mut candidates = self.add_derive_candidates(&types, &derivable, &written);
        self.remove_blocked_derives(&mut candidates);
        self.record_derives(&candidates, &derivable);
    }

    fn type_arities(modules: &[CoreModule]) -> Vec<(BindingId, usize, DefinitionKind)> {
        modules
            .iter()
            .flat_map(|module| &module.items)
            .filter_map(|item| match &item.kind {
                ItemKind::Struct(binding, params, _) => {
                    Some((*binding, params.len(), DefinitionKind::Struct))
                }
                ItemKind::Enum(binding, params, _, _) => {
                    Some((*binding, params.len(), DefinitionKind::Enum))
                }
                _ => None,
            })
            .collect()
    }

    fn derivable_traits(modules: &[CoreModule]) -> Vec<(BindingId, &'static str)> {
        let prelude_traits: Vec<(BindingId, &str)> = modules
            .iter()
            .filter(|module| ModuleSources::is_prelude(&module.path))
            .flat_map(|module| &module.items)
            .filter_map(|item| match &item.kind {
                ItemKind::RustTrait(declaration) => {
                    Some((declaration.binding, declaration.path.as_ref()))
                }
                _ => None,
            })
            .collect();
        StructDerives::DERIVABLE
            .iter()
            .flat_map(|derived| {
                prelude_traits
                    .iter()
                    .filter(move |(_, path)| StructDerives::derive_name(path) == *derived)
                    .map(move |(binding, _)| (*binding, *derived))
            })
            .collect()
    }

    fn written_derives(
        &mut self,
        modules: &[CoreModule],
        derivable: &[(BindingId, &'static str)],
    ) -> HashMap<BindingId, WrittenDerive> {
        let mut written: HashMap<BindingId, WrittenDerive> = HashMap::new();
        for item in modules.iter().flat_map(|module| &module.items) {
            let ItemKind::Derive(struct_binding, name_span, traits) = &item.kind else {
                continue;
            };
            if self.is_rust_enum(*struct_binding) {
                let enum_name = self.bindings.get(*struct_binding).name().to_string();
                self.diagnostics
                    .push(TypeProblems::derive_on_rust_type(&enum_name, *name_span));
                continue;
            }
            if let Some(first) = written.get(struct_binding) {
                let struct_name = self.bindings.get(*struct_binding).name().to_string();
                self.diagnostics.push(TypeProblems::duplicate_derive(
                    &struct_name,
                    item.span,
                    first.span,
                ));
                continue;
            }
            let listed = traits
                .iter()
                .copied()
                .filter(|(trait_binding, span)| {
                    self.accepts_derive(*trait_binding, *span, derivable)
                })
                .collect();
            if let Some(definition) = self.structs.get_mut(*struct_binding) {
                definition.written_derive = Some(item.span);
            }
            if let Some(definition) = self.enums.get_mut(*struct_binding) {
                definition.written_derive = Some(item.span);
            }
            written.insert(
                *struct_binding,
                WrittenDerive {
                    span: item.span,
                    traits: listed,
                },
            );
        }
        written
    }

    fn accepts_derive(
        &mut self,
        trait_binding: BindingId,
        span: Span,
        derivable: &[(BindingId, &'static str)],
    ) -> bool {
        if derivable
            .iter()
            .any(|(binding, _)| *binding == trait_binding)
        {
            return true;
        }
        let names: Vec<String> = derivable
            .iter()
            .map(|(binding, _)| self.bindings.get(*binding).name().to_string())
            .collect();
        let trait_name = self.bindings.get(trait_binding).name().to_string();
        self.diagnostics
            .push(TypeProblems::not_derivable(&trait_name, &names, span));
        false
    }

    fn add_derive_candidates(
        &mut self,
        types: &[(BindingId, usize, DefinitionKind)],
        derivable: &[(BindingId, &'static str)],
        written: &HashMap<BindingId, WrittenDerive>,
    ) -> Vec<DeriveCandidate> {
        let mut candidates = Vec::new();
        for (trait_binding, name) in derivable {
            for (binding, arity, kind) in types {
                if self.is_rust_enum(*binding) {
                    continue;
                }
                let Some(source) = self.derive_source(*binding, name, written, derivable) else {
                    continue;
                };
                let implementor = self.derived_implementor(*binding, *arity, *trait_binding);
                self.traits.add_implementor(*trait_binding, implementor);
                candidates.push(DeriveCandidate {
                    binding: *binding,
                    kind: *kind,
                    trait_binding: *trait_binding,
                    source,
                });
            }
        }
        candidates
    }

    fn is_rust_enum(&self, binding: BindingId) -> bool {
        self.enums
            .get(binding)
            .is_some_and(|definition| definition.rust.is_some())
    }

    fn derive_source(
        &self,
        struct_binding: BindingId,
        name: &str,
        written: &HashMap<BindingId, WrittenDerive>,
        derivable: &[(BindingId, &'static str)],
    ) -> Option<DeriveSource> {
        match written.get(&struct_binding) {
            Some(derive) => Self::listing_span(derive, name, derivable).map(DeriveSource::Listed),
            None if name == StructDerives::COPY
                && self.changed_structs.contains(&struct_binding) =>
            {
                None
            }
            None => StructDerives::INFERRED
                .contains(&name)
                .then_some(DeriveSource::Inferred),
        }
    }

    fn listing_span(
        derive: &WrittenDerive,
        name: &str,
        derivable: &[(BindingId, &'static str)],
    ) -> Option<Span> {
        derive
            .traits
            .iter()
            .find(|(listed, _)| {
                derivable
                    .iter()
                    .find(|(binding, _)| binding == listed)
                    .is_some_and(|(_, listed_name)| StructDerives::brings(listed_name, name))
            })
            .map(|(_, span)| *span)
    }

    fn remove_blocked_derives(&mut self, candidates: &mut Vec<DeriveCandidate>) {
        loop {
            let blocked = self.blocked_derives(candidates);
            if blocked.is_empty() {
                break;
            }
            for (candidate, field, ty) in blocked {
                self.traits
                    .remove_struct_implementor(candidate.trait_binding, candidate.binding);
                candidates.retain(|kept| {
                    (kept.binding, kept.trait_binding)
                        != (candidate.binding, candidate.trait_binding)
                });
                self.report_blocked_derive(candidate, field, ty);
            }
        }
    }

    fn blocked_derives(
        &self,
        candidates: &[DeriveCandidate],
    ) -> Vec<(DeriveCandidate, Rc<str>, Type)> {
        candidates
            .iter()
            .filter_map(|candidate| {
                self.blocking_payload(candidate.binding, candidate.kind, candidate.trait_binding)
                    .map(|(field, ty)| (*candidate, field, ty))
            })
            .collect()
    }

    fn report_blocked_derive(&mut self, candidate: DeriveCandidate, field: Rc<str>, ty: Type) {
        match candidate.source {
            DeriveSource::Listed(span) => self.report_listed_block(candidate, &field, &ty, span),
            DeriveSource::Inferred => self.record_inferred_block(candidate, field, ty),
        }
    }

    fn report_listed_block(
        &mut self,
        candidate: DeriveCandidate,
        field: &Rc<str>,
        ty: &Type,
        span: Span,
    ) {
        let display = self.display();
        let name = self.bindings.get(candidate.binding).name();
        let trait_name = self.bindings.get(candidate.trait_binding).name();
        let diagnostic = match candidate.kind {
            DefinitionKind::Struct => {
                TypeProblems::blocked_derive(name, trait_name, field, &display.show(ty), span)
            }
            DefinitionKind::Enum => {
                TypeProblems::blocked_enum_derive(name, trait_name, field, &display.show(ty), span)
            }
        };
        self.diagnostics.push(diagnostic);
    }

    fn record_inferred_block(&mut self, candidate: DeriveCandidate, field: Rc<str>, ty: Type) {
        match candidate.kind {
            DefinitionKind::Struct => {
                if let Some(definition) = self.structs.get_mut(candidate.binding) {
                    definition
                        .blocked
                        .push((candidate.trait_binding, field, ty));
                }
            }
            DefinitionKind::Enum => {
                if let Some(definition) = self.enums.get_mut(candidate.binding) {
                    definition
                        .blocked
                        .push((candidate.trait_binding, field, ty));
                }
            }
        }
    }

    fn record_derives(
        &mut self,
        candidates: &[DeriveCandidate],
        derivable: &[(BindingId, &'static str)],
    ) {
        for candidate in candidates {
            let name = derivable
                .iter()
                .find(|(binding, _)| *binding == candidate.trait_binding)
                .map(|(_, name)| *name);
            match candidate.kind {
                DefinitionKind::Struct => {
                    if let (Some(name), Some(definition)) =
                        (name, self.structs.get_mut(candidate.binding))
                    {
                        definition.derives.push(name);
                        definition.derived_traits.push(candidate.trait_binding);
                    }
                }
                DefinitionKind::Enum => {
                    if let (Some(name), Some(definition)) =
                        (name, self.enums.get_mut(candidate.binding))
                    {
                        definition.derives.push(name);
                        definition.derived_traits.push(candidate.trait_binding);
                    }
                }
            }
        }
    }

    fn blocking_payload(
        &self,
        binding: BindingId,
        kind: DefinitionKind,
        trait_binding: BindingId,
    ) -> Option<(Rc<str>, Type)> {
        match kind {
            DefinitionKind::Struct => self.blocking_field(binding, trait_binding),
            DefinitionKind::Enum => self.blocking_enum_payload(binding, trait_binding),
        }
    }

    pub(super) fn blocking_enum_payload(
        &self,
        binding: BindingId,
        trait_binding: BindingId,
    ) -> Option<(Rc<str>, Type)> {
        let definition = self.enums.get(binding)?;
        let params: Vec<GenericId> = definition.params.iter().map(|param| param.id).collect();
        definition
            .blocking_variant(&|payload| self.payload_satisfies(trait_binding, payload, &params))
    }

    pub(super) fn blocking_field(
        &self,
        struct_binding: BindingId,
        trait_binding: BindingId,
    ) -> Option<(Rc<str>, Type)> {
        let definition = self.structs.get(struct_binding)?;
        let params: Vec<GenericId> = definition.params.iter().map(|param| param.id).collect();
        definition
            .fields
            .iter()
            .find(|(_, ty)| !self.payload_satisfies(trait_binding, ty, &params))
            .map(|(name, ty)| (name.clone(), ty.clone()))
    }

    fn payload_satisfies(
        &self,
        trait_binding: BindingId,
        payload: &Type,
        params: &[GenericId],
    ) -> bool {
        match payload {
            Type::Function(..) => self.function_field_trait_allowed(trait_binding),
            _ => self
                .traits
                .satisfies(&Bound::plain(trait_binding), payload, &|generic, _| {
                    params.contains(&generic)
                }),
        }
    }

    fn function_field_trait_allowed(&self, trait_binding: BindingId) -> bool {
        self.traits.get(trait_binding).is_some_and(|rust_trait| {
            FunctionFieldTraits::ALLOWED.contains(&StructDerives::derive_name(&rust_trait.path))
        })
    }

    fn derived_implementor(
        &mut self,
        binding: BindingId,
        arity: usize,
        derived: BindingId,
    ) -> Implementor {
        let params: Vec<GenericParam> = (0..arity)
            .map(|position| {
                self.new_generic(&Self::parameter_name(position), vec![Bound::plain(derived)])
            })
            .collect();
        let arguments = params.iter().map(|param| Type::Generic(param.id)).collect();
        Implementor {
            scheme: Scheme {
                lifetimes: Vec::new(),
                params,
                body: Type::Named(binding, arguments),
            },
            argument: None,
        }
    }
}
