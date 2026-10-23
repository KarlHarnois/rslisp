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
        let matrix: Vec<Vec<CoverPattern>> = arms
            .iter()
            .map(|arm| vec![self.cover_pattern(&arm.pattern)])
            .collect();
        if self.covers_all(&matrix, std::slice::from_ref(&scrutinee)) {
            return;
        }
        let missing = self.witnesses(&matrix, &[scrutinee]);
        let mut seen = HashSet::new();
        let cases: Vec<String> = missing
            .iter()
            .map(|row| self.show_witness(&row[0]))
            .filter(|case| seen.insert(case.clone()))
            .collect();
        self.diagnostics
            .push(TypeProblems::non_exhaustive_match(&cases, span));
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

    fn covers_all(&self, matrix: &[Vec<CoverPattern>], types: &[Type]) -> bool {
        if types.is_empty() {
            return !matrix.is_empty();
        }
        match self.constructors(&types[0]) {
            Some(constructors) => constructors.iter().all(|constructor| {
                let specialized = self.specialize(matrix, constructor);
                let mut columns = constructor.arguments.clone();
                columns.extend_from_slice(&types[1..]);
                self.covers_all(&specialized, &columns)
            }),
            None => {
                let default = Self::default_matrix(matrix);
                self.covers_all(&default, &types[1..])
            }
        }
    }

    fn witnesses(&self, matrix: &[Vec<CoverPattern>], types: &[Type]) -> Vec<Vec<Witness>> {
        if types.is_empty() {
            return if matrix.is_empty() {
                vec![Vec::new()]
            } else {
                Vec::new()
            };
        }
        match self.constructors(&types[0]) {
            Some(constructors) => constructors
                .iter()
                .flat_map(|constructor| {
                    let specialized = self.specialize(matrix, constructor);
                    let mut columns = constructor.arguments.clone();
                    columns.extend_from_slice(&types[1..]);
                    self.witnesses(&specialized, &columns)
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
                self.witnesses(&default, &types[1..])
                    .into_iter()
                    .map(|row| {
                        let mut witness = Vec::with_capacity(row.len() + 1);
                        witness.push(Witness::Wild);
                        witness.extend(row);
                        witness
                    })
                    .collect()
            }
        }
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
                            arguments: definition
                                .instantiated_payload(index, &arguments)
                                .unwrap_or_default(),
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
