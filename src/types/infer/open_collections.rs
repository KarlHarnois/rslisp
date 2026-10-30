use std::collections::HashSet;

use crate::core::{BindingId, Expr, PrimitiveType};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::expand::Program;
use crate::source::Span;
use crate::types::checker::TypeChecker;
use crate::types::collection::Collection;
use crate::types::graph::DefinitionGraph;
use crate::types::pinning::InstantiationReport;
use crate::types::problems::{MismatchContext, TypeProblems};
use crate::types::unifier::LiteralKind;
use crate::types::{GenericId, Type, TypedExpr, TypedProgram};

pub(in crate::types) struct OpenSetAt {
    collection: Type,
    key: Type,
    item: Type,
    span: Span,
}

pub(in crate::types) struct OpenCollections {
    generics: Vec<(GenericId, Span)>,
}

impl OpenCollections {
    pub(in crate::types) fn vector_bases(
        &self,
        program: &Program,
        graph: &DefinitionGraph,
        typed: &TypedProgram,
    ) -> Result<HashSet<Span>, Diagnostics> {
        let mut vector_bases = HashSet::new();
        if self.generics.is_empty() {
            return Ok(vector_bases);
        }
        let report = InstantiationReport::of(program, graph, typed);
        let mut diagnostics = Diagnostics::default();
        for (generic, span) in &self.generics {
            let Some(owner) = Self::owner(typed, *generic) else {
                continue;
            };
            if !report.is_used(owner) || Self::is_pinned(&report, owner, *generic) {
                continue;
            }
            if Self::passes_only_sequences(&report, typed, owner, *generic) {
                vector_bases.insert(*span);
            } else {
                diagnostics.push(TypeProblems::unknown_collection(*span));
            }
        }
        if diagnostics.has_errors() {
            return Err(diagnostics);
        }
        Ok(vector_bases)
    }

    fn owner(typed: &TypedProgram, generic: GenericId) -> Option<BindingId> {
        typed
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .find(|definition| {
                definition
                    .scheme
                    .params
                    .iter()
                    .any(|param| param.id == generic)
            })
            .map(|definition| definition.binding)
    }

    fn passes_only_sequences(
        report: &InstantiationReport,
        typed: &TypedProgram,
        owner: BindingId,
        generic: GenericId,
    ) -> bool {
        report.uses_of(owner, generic).iter().all(|used| {
            used.as_ref()
                .and_then(|ty| Collection::of_referent(ty, &typed.rust_types))
                .is_some_and(|collection| collection.is_sequence())
        })
    }

    fn is_pinned(report: &InstantiationReport, owner: BindingId, generic: GenericId) -> bool {
        report
            .one_type(owner)
            .is_some_and(|pinned| pinned.contains_key(&generic))
    }
}

impl TypeChecker {
    pub(super) fn infer_open_set_at(
        &mut self,
        value: TypedExpr,
        keys: &[Expr],
        item: &Expr,
        span: Span,
    ) -> Result<TypedExpr, Diagnostic> {
        let open = OpenSetAt {
            collection: value.ty.clone(),
            key: self.unifier.fresh(),
            item: self.unifier.fresh(),
            span: value.span,
        };
        let parameters = vec![
            Type::mutable(open.collection.clone()),
            open.key.clone(),
            open.item.clone(),
        ];
        let mut arguments = keys.to_vec();
        arguments.push(item.clone());
        self.open_set_ats.push(open);
        self.synthetic_call_with_value(
            "{0}[{1}] = {2}",
            parameters,
            Type::unit(),
            value,
            &arguments,
            span,
        )
    }

    pub(super) fn is_open_collection(&self, value: &TypedExpr, depth: usize) -> bool {
        const ONE_KEY: usize = 1;
        depth == ONE_KEY
            && !self.vector_bases.contains(&value.span)
            && !self.defaults_literal_parameters()
            && matches!(
                self.unifier.resolve(&value.ty),
                Type::Var(var) if self.unifier.literal_kind(var).is_none()
            )
    }

    pub(in crate::types) fn close_open_set_ats(&mut self) {
        for open in std::mem::take(&mut self.open_set_ats) {
            if let Err(diagnostic) = self.close_open_set_at(&open) {
                self.diagnostics.push(diagnostic);
            }
        }
    }

    pub(in crate::types) fn open_collections(&mut self) -> OpenCollections {
        let generics = std::mem::take(&mut self.unsettled_collections)
            .into_iter()
            .filter_map(
                |(collection, span)| match self.unifier.resolve(&collection) {
                    Type::Generic(generic) => Some((generic, span)),
                    _ => None,
                },
            )
            .collect();
        OpenCollections { generics }
    }

    fn close_open_set_at(&mut self, open: &OpenSetAt) -> Result<(), Diagnostic> {
        let resolved = self.unifier.resolve(&open.collection);
        if matches!(resolved, Type::Var(_)) {
            if self.is_integer_literal(&open.key) {
                self.expect_index(open)?;
            }
            self.unsettled_collections
                .push((open.collection.clone(), open.span));
            return Ok(());
        }
        let collection = Collection::of_referent(&resolved, &self.rust_types)
            .filter(|collection| !collection.is_map())
            .ok_or_else(|| {
                TypeProblems::mismatch(
                    "a vector",
                    &self.display().show(&resolved),
                    open.span,
                    MismatchContext::Plain,
                )
            })?;
        let target = self.slice_of(open.item.clone());
        self.expect(&target, collection.ty(), open.span, MismatchContext::Plain)?;
        self.expect_index(open)
    }

    fn expect_index(&mut self, open: &OpenSetAt) -> Result<(), Diagnostic> {
        self.expect(
            &Type::Primitive(PrimitiveType::Usize),
            &open.key,
            open.span,
            MismatchContext::Plain,
        )
    }

    fn is_integer_literal(&self, ty: &Type) -> bool {
        matches!(
            self.unifier.resolve(ty),
            Type::Var(var) if self.unifier.literal_kind(var) == Some(LiteralKind::Integer)
        )
    }
}
