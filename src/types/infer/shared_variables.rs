use crate::diagnostic::Diagnostic;
use crate::types::checker::TypeChecker;
use crate::types::problems::MismatchContext;
use crate::types::unifier::Unifier;
use crate::types::{Borrow, Type, TypeVar, TypedArgument, TypedExpr};

pub(super) struct SharedVariables {
    variables: Vec<Option<TypeVar>>,
}

impl SharedVariables {
    pub(super) fn of(parameters: &[Type], unifier: &Unifier) -> Self {
        let whole: Vec<Option<TypeVar>> = parameters
            .iter()
            .map(|parameter| match unifier.shallow(parameter) {
                Type::Var(var) => Some(var),
                _ => None,
            })
            .collect();
        let variables = whole
            .iter()
            .map(|variable| {
                variable
                    .filter(|var| whole.iter().filter(|other| **other == Some(*var)).count() > 1)
            })
            .collect();
        Self { variables }
    }

    pub(super) fn contains(&self, position: usize) -> bool {
        self.variables[position].is_some()
    }

    fn groups(&self) -> Vec<(TypeVar, Vec<usize>)> {
        let mut groups: Vec<(TypeVar, Vec<usize>)> = Vec::new();
        for (position, variable) in self.variables.iter().enumerate() {
            let Some(var) = variable else {
                continue;
            };
            match groups.iter_mut().find(|(group, _)| group == var) {
                Some((_, positions)) => positions.push(position),
                None => groups.push((*var, vec![position])),
            }
        }
        groups
    }
}

impl TypeChecker {
    pub(super) fn pass_shared_arguments(
        &mut self,
        callee: &TypedExpr,
        shared: &SharedVariables,
        arguments: &mut [TypedArgument],
    ) -> Result<(), Diagnostic> {
        for (var, positions) in shared.groups() {
            let borrowed_by_rust = positions
                .iter()
                .all(|position| self.template_borrows.borrows(callee, *position));
            self.pass_group(var, &positions, borrowed_by_rust, arguments)?;
        }
        Ok(())
    }

    fn pass_group(
        &mut self,
        var: TypeVar,
        positions: &[usize],
        borrowed_by_rust: bool,
        arguments: &mut [TypedArgument],
    ) -> Result<(), Diagnostic> {
        let resolved: Vec<Type> = positions
            .iter()
            .map(|position| self.unifier.resolve(&arguments[*position].value.ty))
            .collect();
        if resolved
            .iter()
            .any(|argument| self.referent_unknown(argument))
        {
            return self.pass_each(var, positions, arguments);
        }
        let referent = self.unify_referents(positions, &resolved, arguments)?;
        let depths: Vec<usize> = resolved.iter().map(Self::reference_depth).collect();
        let target = self.target_depth(var, &referent, &depths, borrowed_by_rust);
        let span = arguments[positions[0]].value.span;
        let chosen = Self::with_references(referent, target);
        self.expect(&Type::Var(var), &chosen, span, MismatchContext::Plain)?;
        for (position, depth) in positions.iter().zip(depths) {
            arguments[*position].borrow = Self::borrow_for_depth(depth, target);
        }
        Ok(())
    }

    fn pass_each(
        &mut self,
        var: TypeVar,
        positions: &[usize],
        arguments: &mut [TypedArgument],
    ) -> Result<(), Diagnostic> {
        for position in positions {
            let borrow = self.pass_argument(&Type::Var(var), &arguments[*position].value)?;
            arguments[*position].borrow = borrow;
        }
        Ok(())
    }

    fn unify_referents(
        &mut self,
        positions: &[usize],
        resolved: &[Type],
        arguments: &[TypedArgument],
    ) -> Result<Type, Diagnostic> {
        let referent = Self::innermost_referent(&resolved[0]).clone();
        for (position, argument) in positions.iter().zip(resolved).skip(1) {
            let span = arguments[*position].value.span;
            let found = Self::innermost_referent(argument);
            self.expect(&referent, found, span, MismatchContext::Plain)?;
        }
        Ok(self.unifier.resolve(&referent))
    }

    fn target_depth(
        &self,
        var: TypeVar,
        referent: &Type,
        depths: &[usize],
        borrowed_by_rust: bool,
    ) -> usize {
        if self.copies_out(var, referent) {
            0
        } else if depths.windows(2).all(|pair| pair[0] == pair[1]) {
            depths[0]
        } else if borrowed_by_rust {
            0
        } else {
            1
        }
    }

    fn copies_out(&self, var: TypeVar, referent: &Type) -> bool {
        match referent {
            Type::Var(literal) => self.unifier.literal_kind(*literal).is_some(),
            _ => self.copy_rules().is_copy(referent) && !self.fails_bounds(var, referent),
        }
    }

    fn with_references(referent: Type, depth: usize) -> Type {
        (0..depth).fold(referent, |inner, _| Type::Ref(Box::new(inner)))
    }

    fn borrow_for_depth(depth: usize, target: usize) -> Borrow {
        if depth == target {
            Borrow::None
        } else if depth < target {
            Borrow::Shared
        } else if target == 0 {
            Borrow::Deref(depth)
        } else {
            Borrow::Collapse(depth - target)
        }
    }
}
