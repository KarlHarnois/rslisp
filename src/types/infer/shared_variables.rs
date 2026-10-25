use std::collections::HashMap;

use crate::diagnostic::Diagnostic;
use crate::types::checker::TypeChecker;
use crate::types::problems::MismatchContext;
use crate::types::unifier::Unifier;
use crate::types::{Borrow, Bound, Type, TypeVar, TypedArgument, TypedExpr};

pub(super) struct SharedVariables {
    groups: Vec<SharedGroup>,
}

struct SharedGroup {
    root: TypeVar,
    members: Vec<SharedMember>,
}

struct SharedMember {
    var: TypeVar,
    positions: Vec<usize>,
}

impl SharedGroup {
    fn is_linked(&self) -> bool {
        self.members.len() > 1
    }

    fn positions(&self) -> impl Iterator<Item = usize> + '_ {
        self.members
            .iter()
            .flat_map(|member| member.positions.iter().copied())
    }

    fn contains(&self, position: usize) -> bool {
        self.members
            .iter()
            .any(|member| member.positions.contains(&position))
    }

    fn add_position(&mut self, var: TypeVar, position: usize) {
        match self.members.iter_mut().find(|member| member.var == var) {
            Some(member) => member.positions.push(position),
            None => self.members.push(SharedMember {
                var,
                positions: vec![position],
            }),
        }
    }
}

#[derive(Default)]
struct VariableLinks {
    parents: HashMap<TypeVar, TypeVar>,
}

impl VariableLinks {
    fn link(&mut self, left: TypeVar, right: TypeVar) {
        let left = self.find(left);
        let right = self.find(right);
        if left != right {
            self.parents.insert(left, right);
        }
    }

    fn find(&mut self, var: TypeVar) -> TypeVar {
        let parent = *self.parents.entry(var).or_insert(var);
        if parent == var {
            return var;
        }
        let root = self.find(parent);
        self.parents.insert(var, root);
        root
    }
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
        let mut links = Self::links(&whole, unifier);
        let mut groups: Vec<SharedGroup> = Vec::new();
        for (position, whole) in whole.iter().enumerate() {
            let Some(var) = whole else {
                continue;
            };
            let root = links.find(*var);
            let group = Self::group_of(&mut groups, root);
            group.add_position(*var, position);
        }
        groups.retain(|group| group.positions().count() > 1);
        Self { groups }
    }

    fn links(whole: &[Option<TypeVar>], unifier: &Unifier) -> VariableLinks {
        let mut links = VariableLinks::default();
        for var in whole.iter().flatten() {
            for bound in unifier.bounds(*var) {
                let argument = bound
                    .argument
                    .as_ref()
                    .map(|argument| unifier.shallow(argument));
                if let Some(Type::Var(linked)) = argument {
                    links.link(*var, linked);
                }
            }
        }
        links
    }

    fn group_of(groups: &mut Vec<SharedGroup>, root: TypeVar) -> &mut SharedGroup {
        match groups.iter().position(|group| group.root == root) {
            Some(position) => &mut groups[position],
            None => {
                groups.push(SharedGroup {
                    root,
                    members: Vec::new(),
                });
                groups.last_mut().expect("the group was just added")
            }
        }
    }

    pub(super) fn contains(&self, position: usize) -> bool {
        self.groups.iter().any(|group| group.contains(position))
    }

    fn groups(&self) -> &[SharedGroup] {
        &self.groups
    }
}

impl TypeChecker {
    pub(super) fn pass_shared_arguments(
        &mut self,
        callee: Option<&TypedExpr>,
        shared: &SharedVariables,
        arguments: &mut [TypedArgument],
    ) -> Result<(), Diagnostic> {
        for group in shared.groups() {
            if group
                .positions()
                .any(|position| arguments[position].value.is_written_form())
            {
                self.pass_each(callee, group, arguments)?;
                continue;
            }
            if group.is_linked() && self.try_as_written(group, arguments)? {
                continue;
            }
            let borrowed_by_rust = callee.is_some_and(|callee| {
                group
                    .positions()
                    .all(|position| self.template_borrows.borrows(callee, position))
            });
            self.pass_group(callee, group, borrowed_by_rust, arguments)?;
        }
        Ok(())
    }

    fn try_as_written(
        &mut self,
        group: &SharedGroup,
        arguments: &mut [TypedArgument],
    ) -> Result<bool, Diagnostic> {
        let Some(assignment) = self.written_assignment(group, arguments) else {
            return Ok(false);
        };
        if !self.written_depths_agree(group, arguments)
            || !self.written_types_hold(group, &assignment)
        {
            return Ok(false);
        }
        for member in &group.members {
            let span = arguments[member.positions[0]].value.span;
            let found = assignment[&member.var].clone();
            self.expect(&Type::Var(member.var), &found, span, MismatchContext::Plain)?;
            for position in &member.positions {
                arguments[*position].borrow = Borrow::None;
            }
        }
        Ok(true)
    }

    fn written_depths_agree(&self, group: &SharedGroup, arguments: &[TypedArgument]) -> bool {
        let resolved: Vec<Type> = group
            .positions()
            .map(|position| self.unifier.resolve(&arguments[position].value.ty))
            .collect();
        let referent = Self::innermost_referent(&resolved[0]).clone();
        if !resolved
            .iter()
            .all(|argument| Self::innermost_referent(argument) == &referent)
        {
            return true;
        }
        let depths: Vec<usize> = group
            .positions()
            .map(|position| self.reference_depth_as_written(&arguments[position]))
            .collect();
        depths.windows(2).all(|pair| pair[0] == pair[1])
    }

    fn written_types_hold(&self, group: &SharedGroup, assignment: &HashMap<TypeVar, Type>) -> bool {
        !assignment
            .values()
            .any(|argument| self.referent_unknown(argument))
            && self.group_bounds_hold(group, assignment)
    }

    fn written_assignment(
        &self,
        group: &SharedGroup,
        arguments: &[TypedArgument],
    ) -> Option<HashMap<TypeVar, Type>> {
        let mut assignment = HashMap::new();
        for member in &group.members {
            let first = self
                .unifier
                .resolve(&arguments[member.positions[0]].value.ty);
            let agrees = member
                .positions
                .iter()
                .all(|position| self.unifier.resolve(&arguments[*position].value.ty) == first);
            if !agrees {
                return None;
            }
            assignment.insert(member.var, first);
        }
        Some(assignment)
    }

    fn group_bounds_hold(&self, group: &SharedGroup, assignment: &HashMap<TypeVar, Type>) -> bool {
        group.members.iter().all(|member| {
            let implementor = &assignment[&member.var];
            self.unifier
                .bounds(member.var)
                .iter()
                .all(|bound| self.bound_holds(bound, implementor, assignment))
        })
    }

    fn pass_group(
        &mut self,
        callee: Option<&TypedExpr>,
        group: &SharedGroup,
        borrowed_by_rust: bool,
        arguments: &mut [TypedArgument],
    ) -> Result<(), Diagnostic> {
        let positions: Vec<usize> = group.positions().collect();
        let resolved: Vec<Type> = positions
            .iter()
            .map(|position| self.unifier.resolve(&arguments[*position].value.ty))
            .collect();
        if resolved
            .iter()
            .any(|argument| self.referent_unknown(argument))
        {
            return self.pass_each(callee, group, arguments);
        }
        let referent = self.unify_referents(&positions, &resolved, arguments)?;
        let depths: Vec<usize> = positions
            .iter()
            .map(|position| self.reference_depth_as_written(&arguments[*position]))
            .collect();
        let target = self.target_depth(group, &referent, &depths, borrowed_by_rust);
        let span = arguments[positions[0]].value.span;
        let chosen = Self::with_references(referent, target);
        for member in &group.members {
            self.expect(
                &Type::Var(member.var),
                &chosen,
                span,
                MismatchContext::Plain,
            )?;
        }
        for (position, depth) in positions.iter().zip(depths) {
            arguments[*position].borrow = Self::borrow_for_depth(depth, target);
        }
        Ok(())
    }

    fn pass_each(
        &mut self,
        callee: Option<&TypedExpr>,
        group: &SharedGroup,
        arguments: &mut [TypedArgument],
    ) -> Result<(), Diagnostic> {
        let root = group.root;
        let span = arguments[group.positions().next().expect("a group has a position")]
            .value
            .span;
        for member in &group.members {
            self.expect(
                &Type::Var(root),
                &Type::Var(member.var),
                span,
                MismatchContext::Plain,
            )?;
        }
        for position in group.positions() {
            let borrow = self.pass_call_argument(
                callee,
                position,
                &Type::Var(root),
                &arguments[position].value,
            )?;
            arguments[position].borrow = borrow;
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
        group: &SharedGroup,
        referent: &Type,
        depths: &[usize],
        borrowed_by_rust: bool,
    ) -> usize {
        if self.copies_out(group, referent) {
            0
        } else if depths.windows(2).all(|pair| pair[0] == pair[1]) {
            let depth = depths[0];
            if self.reference_blocks_bounds(group, referent, depth) {
                0
            } else {
                depth
            }
        } else if borrowed_by_rust {
            0
        } else {
            1
        }
    }

    fn reference_blocks_bounds(&self, group: &SharedGroup, referent: &Type, depth: usize) -> bool {
        depth > 0
            && !self.group_fails_bounds(group, referent)
            && self.group_fails_bounds(group, &Self::with_references(referent.clone(), depth))
    }

    fn copies_out(&self, group: &SharedGroup, referent: &Type) -> bool {
        match referent {
            Type::Var(literal) => self.unifier.literal_kind(*literal).is_some(),
            _ => {
                self.copy_rules().is_small_copy(referent)
                    && !self.group_fails_bounds(group, referent)
            }
        }
    }

    fn group_fails_bounds(&self, group: &SharedGroup, referent: &Type) -> bool {
        let assignment: HashMap<TypeVar, Type> = group
            .members
            .iter()
            .map(|member| (member.var, referent.clone()))
            .collect();
        group.members.iter().any(|member| {
            self.unifier
                .bounds(member.var)
                .iter()
                .any(|bound| !self.bound_holds(bound, referent, &assignment))
        })
    }

    fn bound_holds(
        &self,
        bound: &Bound,
        implementor: &Type,
        assignment: &HashMap<TypeVar, Type>,
    ) -> bool {
        let bound = bound.substituted_vars(assignment);
        self.traits
            .satisfies(&bound, implementor, &|generic, required| {
                self.unifier.generic_bounds(generic).contains(required)
            })
    }

    fn reference_depth_as_written(&self, argument: &TypedArgument) -> usize {
        let written = self.place_type(&argument.value);
        Self::reference_depth(&self.unifier.resolve(&written))
    }

    fn with_references(referent: Type, depth: usize) -> Type {
        (0..depth).fold(referent, |inner, _| Type::shared(inner))
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
