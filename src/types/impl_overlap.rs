use rustc_hash::FxHashMap;

use super::ty::{GenericId, Type};

#[derive(Default)]
pub struct ImplementorOverlap {
    bindings: FxHashMap<GenericId, Type>,
}

impl ImplementorOverlap {
    pub fn overlaps(first: &Type, second: &Type) -> bool {
        Self::default().unify(first, second)
    }

    fn unify(&mut self, first: &Type, second: &Type) -> bool {
        let first = self.resolved(first);
        let second = self.resolved(second);
        match (&first, &second) {
            (Type::Generic(left), Type::Generic(right)) if left == right => true,
            (Type::Generic(generic), other) | (other, Type::Generic(generic)) => {
                if self.occurs(*generic, other) {
                    return false;
                }
                self.bindings.insert(*generic, other.clone());
                true
            }
            _ => {
                first.shares_head_with(&second)
                    && first
                        .parts()
                        .into_iter()
                        .zip(second.parts())
                        .all(|(left, right)| self.unify(left, right))
            }
        }
    }

    fn occurs(&self, generic: GenericId, ty: &Type) -> bool {
        let resolved = self.resolved(ty);
        resolved == Type::Generic(generic)
            || resolved
                .parts()
                .into_iter()
                .any(|part| self.occurs(generic, part))
    }

    fn resolved(&self, ty: &Type) -> Type {
        match ty {
            Type::Generic(generic) => self
                .bindings
                .get(generic)
                .map_or_else(|| ty.clone(), |bound| self.resolved(bound)),
            _ => ty.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{BindingArena, BindingId, BindingInfo, BindingKind, PrimitiveType};

    struct Types {
        tree: BindingId,
        pair: BindingId,
    }

    impl Types {
        fn new() -> Self {
            let mut bindings = BindingArena::default();
            let mut add = |name: &str| {
                bindings.add(BindingInfo::new(
                    name.into(),
                    BindingKind::TypeVariable,
                    None,
                ))
            };
            Self {
                tree: add("tree"),
                pair: add("pair"),
            }
        }

        fn tree(&self, argument: Type) -> Type {
            Type::Named(self.tree, vec![argument])
        }

        fn pair(&self, first: Type, second: Type) -> Type {
            Type::Named(self.pair, vec![first, second])
        }

        fn generic(index: usize) -> Type {
            Type::Generic(GenericId::new(index))
        }

        fn integer() -> Type {
            Type::Primitive(PrimitiveType::I64)
        }

        fn boolean() -> Type {
            Type::Primitive(PrimitiveType::Bool)
        }
    }

    #[test]
    fn a_generic_implementor_overlaps_an_instance() {
        let types = Types::new();
        assert!(ImplementorOverlap::overlaps(
            &types.tree(Types::generic(0)),
            &types.tree(Types::integer())
        ));
    }

    #[test]
    fn two_instances_overlap_only_when_equal() {
        let types = Types::new();
        assert!(!ImplementorOverlap::overlaps(
            &types.tree(Types::integer()),
            &types.tree(Types::boolean())
        ));
        assert!(ImplementorOverlap::overlaps(
            &types.tree(Types::integer()),
            &types.tree(Types::integer())
        ));
    }

    #[test]
    fn crossed_generics_overlap() {
        let types = Types::new();
        assert!(ImplementorOverlap::overlaps(
            &types.pair(Types::generic(0), Types::integer()),
            &types.pair(Types::boolean(), Types::generic(1))
        ));
    }

    #[test]
    fn a_repeated_generic_keeps_its_binding() {
        let types = Types::new();
        assert!(!ImplementorOverlap::overlaps(
            &types.pair(Types::generic(0), Types::generic(0)),
            &types.pair(Types::integer(), Types::boolean())
        ));
    }

    #[test]
    fn a_generic_never_binds_a_type_that_holds_it() {
        let types = Types::new();
        assert!(!ImplementorOverlap::overlaps(
            &types.pair(Types::generic(0), Types::generic(0)),
            &types.pair(Types::generic(1), types.tree(Types::generic(1)))
        ));
    }

    #[test]
    fn a_repeated_generic_never_binds_through_a_longer_chain() {
        let types = Types::new();
        let first = types.pair(
            Types::generic(0),
            types.pair(Types::generic(0), Types::generic(0)),
        );
        let second = types.pair(
            Types::generic(1),
            types.pair(types.tree(Types::generic(1)), Types::generic(1)),
        );
        assert!(!ImplementorOverlap::overlaps(&first, &second));
    }

    #[test]
    fn one_generic_on_both_sides_overlaps_itself() {
        let types = Types::new();
        assert!(ImplementorOverlap::overlaps(
            &types.tree(Types::generic(0)),
            &types.tree(Types::generic(0))
        ));
    }
}
