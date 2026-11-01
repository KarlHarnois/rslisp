use std::collections::BTreeSet;

use super::open_literals::TypeContext;
use super::type_anchor::TypeAnchor;
use crate::core::{BindingId, TypeExpr, TypeExprKind};
use crate::types::{GenericParam, Type, TypedExpr};

type Generics = BTreeSet<usize>;

pub(super) struct GenericPositions {
    parameters: Vec<Generics>,
    result: Generics,
}

impl GenericPositions {
    pub(super) fn of_function(generics: &[GenericParam], function: &Type) -> Option<Self> {
        let Type::Function(_, parameters, result) = function else {
            return None;
        };
        let mentioned = |ty: &Type| Self::indexes(generics, |generic| ty.mentions(generic.id));
        let links: Vec<Generics> = generics
            .iter()
            .map(|generic| {
                generic
                    .bounds
                    .iter()
                    .filter_map(|bound| bound.argument.as_ref())
                    .flat_map(mentioned)
                    .collect()
            })
            .collect();
        let parameters = parameters.iter().map(mentioned).collect();
        Some(Self::linked(parameters, mentioned(result), &links))
    }

    pub(super) fn of_fields<'t>(
        generics: &[GenericParam],
        fields: impl Iterator<Item = &'t Type>,
    ) -> Self {
        Self {
            parameters: fields
                .map(|ty| Self::indexes(generics, |generic| ty.mentions(generic.id)))
                .collect(),
            result: (0..generics.len()).collect(),
        }
    }

    pub(super) fn of_template(type_expr: &TypeExpr) -> Option<Self> {
        let TypeExprKind::Where(binders, body) = type_expr.kind() else {
            return None;
        };
        let TypeExprKind::Function(_, parameters, result) = body.kind() else {
            return None;
        };
        let mentioned = |type_expr: &TypeExpr| {
            Self::indexes(binders, |binder| {
                Self::names_variable(type_expr, binder.variable)
            })
        };
        let links: Vec<Generics> = binders
            .iter()
            .map(|binder| {
                binder
                    .bounds
                    .iter()
                    .filter_map(|bound| bound.argument.as_ref())
                    .flat_map(mentioned)
                    .collect()
            })
            .collect();
        let parameters = parameters.iter().map(mentioned).collect();
        Some(Self::linked(parameters, mentioned(result), &links))
    }

    fn indexes<T>(generics: &[T], mentions: impl Fn(&T) -> bool) -> Generics {
        generics
            .iter()
            .enumerate()
            .filter(|(_, generic)| mentions(generic))
            .map(|(index, _)| index)
            .collect()
    }

    fn names_variable(type_expr: &TypeExpr, variable: BindingId) -> bool {
        matches!(type_expr.kind(), TypeExprKind::Named(binding, _) if *binding == variable)
            || type_expr
                .parts()
                .into_iter()
                .any(|part| Self::names_variable(part, variable))
    }

    fn linked(parameters: Vec<Generics>, result: Generics, links: &[Generics]) -> Self {
        let with_links = |generics: &Generics| -> Generics {
            generics
                .iter()
                .flat_map(|generic| Self::linked_to(*generic, links))
                .collect()
        };
        Self {
            parameters: parameters.iter().map(with_links).collect(),
            result: with_links(&result),
        }
    }

    fn linked_to(generic: usize, links: &[Generics]) -> Generics {
        let linking = (0..links.len()).filter(|other| links[*other].contains(&generic));
        std::iter::once(generic)
            .chain(links[generic].iter().copied())
            .chain(linking)
            .collect()
    }

    pub(super) fn contexts(&self, values: &[&TypedExpr], outer: TypeContext) -> Vec<TypeContext> {
        (0..values.len())
            .map(|position| {
                if self.is_open(position, values, outer) {
                    TypeContext::Unknown
                } else {
                    TypeContext::Known
                }
            })
            .collect()
    }

    fn is_open(&self, position: usize, values: &[&TypedExpr], outer: TypeContext) -> bool {
        self.parameters.get(position).is_some_and(|generics| {
            generics
                .iter()
                .any(|generic| !self.is_fixed(*generic, position, values, outer))
        })
    }

    fn is_fixed(
        &self,
        generic: usize,
        position: usize,
        values: &[&TypedExpr],
        outer: TypeContext,
    ) -> bool {
        let by_result = outer == TypeContext::Known && self.result.contains(&generic);
        by_result || self.anchor(generic, values) != Some(position)
    }

    fn anchor(&self, generic: usize, values: &[&TypedExpr]) -> Option<usize> {
        TypeAnchor::among(
            self.parameters
                .iter()
                .zip(values)
                .enumerate()
                .filter(|(_, (generics, _))| generics.contains(&generic))
                .map(|(position, (_, value))| (position, *value)),
        )
    }
}
