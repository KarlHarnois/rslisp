use std::collections::HashSet;

use super::checker::TypeChecker;
use super::problems::TypeProblems;
use crate::core::{BindingId, BindingKind, Literal, PrimitiveType};
use crate::source::Span;
use crate::types::{Type, TypedArm, TypedPattern, TypedPatternKind};

#[derive(Clone)]
enum CoverPattern {
    Wild,
    Variant {
        owner: BindingId,
        index: usize,
        arguments: Vec<CoverPattern>,
    },
    Literal(Literal),
}

#[derive(Clone)]
enum Witness {
    Wild,
    Variant {
        owner: BindingId,
        index: usize,
        arguments: Vec<Witness>,
    },
    Bool(bool),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tag {
    Variant { owner: BindingId, index: usize },
    Bool(bool),
}

struct Constructor {
    tag: Tag,
    arguments: Vec<Type>,
}

impl TypeChecker {
    pub(super) fn check_match_coverage(&mut self, span: Span, scrutinee: &Type, arms: &[TypedArm]) {
        let (scrutinee, _) = self.peeled_reference(scrutinee);
        let mut matrix: Vec<Vec<CoverPattern>> = Vec::new();
        for (position, arm) in arms.iter().enumerate() {
            let pattern = self.cover_pattern(&arm.pattern);
            let query = [pattern.clone()];
            if self.useful(
                &matrix,
                &query,
                std::slice::from_ref(&scrutinee),
                &mut Vec::new(),
            ) {
                matrix.push(vec![pattern]);
            } else {
                let covered_by = self.covering_arm(&arms[..position], &pattern, &scrutinee);
                self.warnings
                    .push(TypeProblems::unreachable_arm(arm.pattern.span, covered_by));
            }
        }
        let exhaustive = [CoverPattern::Wild];
        if !self.useful(
            &matrix,
            &exhaustive,
            std::slice::from_ref(&scrutinee),
            &mut Vec::new(),
        ) {
            return;
        }
        let missing = self.witnesses(&matrix, &[scrutinee], &mut Vec::new());
        let mut seen = HashSet::new();
        let cases: Vec<String> = missing
            .iter()
            .map(|row| self.show_witness(&row[0]))
            .filter(|case| seen.insert(case.clone()))
            .collect();
        self.diagnostics
            .push(TypeProblems::non_exhaustive_match(&cases, span));
    }

    pub(super) fn match_is_exhaustive_without_guards(
        &self,
        scrutinee: &Type,
        arms: &[TypedArm],
    ) -> bool {
        let (scrutinee, _) = self.peeled_reference(scrutinee);
        let matrix: Vec<Vec<CoverPattern>> = arms
            .iter()
            .filter(|arm| !self.pattern_has_guard(&arm.pattern))
            .map(|arm| vec![self.cover_pattern(&arm.pattern)])
            .collect();
        !self.useful(
            &matrix,
            &[CoverPattern::Wild],
            std::slice::from_ref(&scrutinee),
            &mut Vec::new(),
        )
    }

    fn pattern_has_guard(&self, pattern: &TypedPattern) -> bool {
        if pattern.boxed
            && matches!(
                &pattern.kind,
                TypedPatternKind::Variant(..) | TypedPatternKind::Literal(..)
            )
        {
            return true;
        }
        match &pattern.kind {
            TypedPatternKind::Wildcard | TypedPatternKind::Bind(_) => false,
            TypedPatternKind::Variant(_, arguments) => arguments
                .iter()
                .any(|argument| self.pattern_has_guard(argument)),
            TypedPatternKind::Literal(literal, ty) => {
                matches!(literal, Literal::String(_))
                    && matches!(ty, Type::Primitive(PrimitiveType::String))
            }
        }
    }

    fn cover_pattern(&self, pattern: &TypedPattern) -> CoverPattern {
        match &pattern.kind {
            TypedPatternKind::Wildcard | TypedPatternKind::Bind(_) => CoverPattern::Wild,
            TypedPatternKind::Variant(binding, arguments) => {
                let BindingKind::Variant(_, info) = self.bindings.get(*binding).kind() else {
                    unreachable!("a typed variant pattern names a variant binding");
                };
                CoverPattern::Variant {
                    owner: info.owner,
                    index: info.index,
                    arguments: arguments
                        .iter()
                        .map(|argument| self.cover_pattern(argument))
                        .collect(),
                }
            }
            TypedPatternKind::Literal(literal, _) => CoverPattern::Literal(literal.clone()),
        }
    }

    fn useful(
        &self,
        matrix: &[Vec<CoverPattern>],
        query: &[CoverPattern],
        types: &[Type],
        expanding: &mut Vec<BindingId>,
    ) -> bool {
        if query.is_empty() {
            return matrix.is_empty();
        }
        if matrix.is_empty() {
            return true;
        }
        if matrix.iter().any(|row| {
            row.iter()
                .all(|pattern| matches!(pattern, CoverPattern::Wild))
        }) {
            return false;
        }
        match &query[0] {
            CoverPattern::Wild => self.useful_wild(matrix, query, types, expanding),
            CoverPattern::Variant { .. } => self.useful_variant(matrix, query, types, expanding),
            CoverPattern::Literal(literal) => {
                self.useful_literal(matrix, literal, query, types, expanding)
            }
        }
    }

    fn useful_wild(
        &self,
        matrix: &[Vec<CoverPattern>],
        query: &[CoverPattern],
        types: &[Type],
        expanding: &mut Vec<BindingId>,
    ) -> bool {
        let owner = self.enum_owner(&types[0]);
        if let Some(owner) = owner
            && expanding.contains(&owner)
        {
            return self.useful_recursive_wild(matrix, query, types, expanding);
        }
        if let Some(owner) = owner {
            expanding.push(owner);
        }
        let result = match self.constructors(&types[0]) {
            Some(constructors) => constructors.iter().any(|constructor| {
                self.useful_constructor(matrix, query, types, constructor, expanding)
            }),
            None => {
                let default = Self::default_matrix(matrix);
                self.useful(&default, &query[1..], &types[1..], expanding)
            }
        };
        if owner.is_some() {
            expanding.pop();
        }
        result
    }

    fn useful_constructor(
        &self,
        matrix: &[Vec<CoverPattern>],
        query: &[CoverPattern],
        types: &[Type],
        constructor: &Constructor,
        expanding: &mut Vec<BindingId>,
    ) -> bool {
        let specialized = self.specialize(matrix, constructor);
        let mut row: Vec<CoverPattern> = constructor
            .arguments
            .iter()
            .map(|_| CoverPattern::Wild)
            .collect();
        row.extend_from_slice(&query[1..]);
        let mut columns = constructor.arguments.clone();
        columns.extend_from_slice(&types[1..]);
        self.useful(&specialized, &row, &columns, expanding)
    }

    fn useful_recursive_wild(
        &self,
        matrix: &[Vec<CoverPattern>],
        query: &[CoverPattern],
        types: &[Type],
        expanding: &mut Vec<BindingId>,
    ) -> bool {
        let mut default = Vec::new();
        let mut concrete = Vec::new();
        for row in matrix {
            match row.first() {
                Some(CoverPattern::Wild) => default.push(row[1..].to_vec()),
                _ => concrete.push(row.clone()),
            }
        }
        if !default.is_empty() && !self.useful(&default, &query[1..], &types[1..], expanding) {
            return false;
        }
        self.constructors(&types[0])
            .unwrap_or_default()
            .iter()
            .any(|constructor| {
                self.useful_constructor(&concrete, query, types, constructor, expanding)
            })
    }

    fn useful_variant(
        &self,
        matrix: &[Vec<CoverPattern>],
        query: &[CoverPattern],
        types: &[Type],
        expanding: &mut Vec<BindingId>,
    ) -> bool {
        let CoverPattern::Variant {
            owner,
            index,
            arguments,
        } = &query[0]
        else {
            unreachable!("the caller matches a variant query");
        };
        let tag = Tag::Variant {
            owner: *owner,
            index: *index,
        };
        let pushed = self
            .enum_owner(&types[0])
            .filter(|owner| !expanding.contains(owner));
        if let Some(owner) = pushed {
            expanding.push(owner);
        }
        let constructor = self.constructors(&types[0]).and_then(|constructors| {
            constructors
                .into_iter()
                .find(|constructor| constructor.tag == tag)
        });
        let result = match constructor {
            Some(constructor) => {
                let specialized = self.specialize(matrix, &constructor);
                let mut row = arguments.clone();
                row.extend_from_slice(&query[1..]);
                let mut columns = constructor.arguments.clone();
                columns.extend_from_slice(&types[1..]);
                self.useful(&specialized, &row, &columns, expanding)
            }
            None => false,
        };
        if pushed.is_some() {
            expanding.pop();
        }
        result
    }

    fn useful_literal(
        &self,
        matrix: &[Vec<CoverPattern>],
        literal: &Literal,
        query: &[CoverPattern],
        types: &[Type],
        expanding: &mut Vec<BindingId>,
    ) -> bool {
        let specialized = Self::specialize_literal(matrix, literal);
        self.useful(&specialized, &query[1..], &types[1..], expanding)
    }

    fn expansion(
        &self,
        ty: &Type,
        expanding: &[BindingId],
    ) -> (Option<BindingId>, Option<Vec<Constructor>>) {
        match self.enum_owner(ty) {
            Some(owner) if !expanding.contains(&owner) => (Some(owner), self.constructors(ty)),
            Some(_) => (None, None),
            None => (None, self.constructors(ty)),
        }
    }

    fn enum_owner(&self, ty: &Type) -> Option<BindingId> {
        let (ty, _) = self.peeled_reference(ty);
        let Type::Named(owner, _) = &ty else {
            return None;
        };
        self.enums.get(*owner).map(|_| *owner)
    }

    fn covering_arm(
        &self,
        previous: &[TypedArm],
        pattern: &CoverPattern,
        scrutinee: &Type,
    ) -> Option<Span> {
        previous
            .iter()
            .find(|arm| {
                !self.useful(
                    &[vec![self.cover_pattern(&arm.pattern)]],
                    std::slice::from_ref(pattern),
                    std::slice::from_ref(scrutinee),
                    &mut Vec::new(),
                )
            })
            .map(|arm| arm.pattern.span)
    }

    fn witnesses(
        &self,
        matrix: &[Vec<CoverPattern>],
        types: &[Type],
        expanding: &mut Vec<BindingId>,
    ) -> Vec<Vec<Witness>> {
        if types.is_empty() {
            return if matrix.is_empty() {
                vec![Vec::new()]
            } else {
                Vec::new()
            };
        }
        let (owner, constructors) = self.expansion(&types[0], expanding);
        let pushed = owner.is_some();
        if let Some(owner) = owner {
            expanding.push(owner);
        }
        let result = match constructors {
            Some(constructors) => constructors
                .iter()
                .flat_map(|constructor| {
                    let specialized = self.specialize(matrix, constructor);
                    let mut columns = constructor.arguments.clone();
                    columns.extend_from_slice(&types[1..]);
                    self.witnesses(&specialized, &columns, expanding)
                        .into_iter()
                        .map(|row| {
                            let (arguments, rest) = row.split_at(constructor.arguments.len());
                            let mut witness = Vec::with_capacity(rest.len() + 1);
                            witness.push(Self::constructor_witness(constructor, arguments));
                            witness.extend_from_slice(rest);
                            witness
                        })
                        .collect::<Vec<_>>()
                })
                .collect(),
            None => {
                let default = Self::default_matrix(matrix);
                self.witnesses(&default, &types[1..], expanding)
                    .into_iter()
                    .map(|row| {
                        let mut witness = Vec::with_capacity(row.len() + 1);
                        witness.push(Witness::Wild);
                        witness.extend(row);
                        witness
                    })
                    .collect()
            }
        };
        if pushed {
            expanding.pop();
        }
        result
    }

    fn constructors(&self, ty: &Type) -> Option<Vec<Constructor>> {
        let (ty, _) = self.peeled_reference(ty);
        match ty {
            Type::Primitive(PrimitiveType::Bool) => Some(vec![
                Constructor {
                    tag: Tag::Bool(false),
                    arguments: Vec::new(),
                },
                Constructor {
                    tag: Tag::Bool(true),
                    arguments: Vec::new(),
                },
            ]),
            Type::Named(owner, arguments) => {
                let definition = self.enums.get(owner)?;
                Some(
                    definition
                        .variants
                        .iter()
                        .enumerate()
                        .map(|(index, _)| Constructor {
                            tag: Tag::Variant { owner, index },
                            arguments: self.visible_payloads(owner, index, &arguments),
                        })
                        .collect(),
                )
            }
            _ => None,
        }
    }

    fn specialize(
        &self,
        matrix: &[Vec<CoverPattern>],
        constructor: &Constructor,
    ) -> Vec<Vec<CoverPattern>> {
        let mut specialized = Vec::new();
        for row in matrix {
            let Some(first) = row.first() else {
                continue;
            };
            let expanded = match first {
                CoverPattern::Wild => {
                    let mut expanded = vec![CoverPattern::Wild; constructor.arguments.len()];
                    expanded.extend_from_slice(&row[1..]);
                    Some(expanded)
                }
                CoverPattern::Variant {
                    owner,
                    index,
                    arguments,
                } if constructor.tag
                    == (Tag::Variant {
                        owner: *owner,
                        index: *index,
                    }) =>
                {
                    let mut expanded = arguments.clone();
                    expanded.extend_from_slice(&row[1..]);
                    Some(expanded)
                }
                CoverPattern::Literal(Literal::Bool(value))
                    if constructor.tag == Tag::Bool(*value) =>
                {
                    Some(row[1..].to_vec())
                }
                _ => None,
            };
            if let Some(expanded) = expanded {
                specialized.push(expanded);
            }
        }
        specialized
    }

    fn specialize_literal(
        matrix: &[Vec<CoverPattern>],
        literal: &Literal,
    ) -> Vec<Vec<CoverPattern>> {
        matrix
            .iter()
            .filter_map(|row| match row.first()? {
                CoverPattern::Wild => Some(row[1..].to_vec()),
                CoverPattern::Literal(other) if Self::same_literal(other, literal) => {
                    Some(row[1..].to_vec())
                }
                _ => None,
            })
            .collect()
    }

    fn same_literal(left: &Literal, right: &Literal) -> bool {
        match (left, right) {
            (Literal::Number(left), Literal::Number(right)) => left.value() == right.value(),
            (Literal::String(left), Literal::String(right)) => left == right,
            (Literal::Bool(left), Literal::Bool(right)) => left == right,
            _ => false,
        }
    }

    fn default_matrix(matrix: &[Vec<CoverPattern>]) -> Vec<Vec<CoverPattern>> {
        matrix
            .iter()
            .filter(|row| matches!(row.first(), Some(CoverPattern::Wild)))
            .map(|row| row[1..].to_vec())
            .collect()
    }

    fn constructor_witness(constructor: &Constructor, arguments: &[Witness]) -> Witness {
        match constructor.tag {
            Tag::Variant { owner, index } => Witness::Variant {
                owner,
                index,
                arguments: arguments.to_vec(),
            },
            Tag::Bool(value) => Witness::Bool(value),
        }
    }

    fn show_witness(&self, witness: &Witness) -> String {
        match witness {
            Witness::Wild => "_".to_string(),
            Witness::Bool(value) => value.to_string(),
            Witness::Variant {
                owner,
                index,
                arguments,
            } => {
                let name = self
                    .enums
                    .get(*owner)
                    .and_then(|definition| definition.variants.get(*index))
                    .map(|variant| variant.name.clone())
                    .unwrap_or_default();
                if arguments.is_empty() {
                    return name.to_string();
                }
                let parts: Vec<String> = arguments
                    .iter()
                    .map(|argument| self.show_witness(argument))
                    .collect();
                format!("({name} {})", parts.join(" "))
            }
        }
    }
}
