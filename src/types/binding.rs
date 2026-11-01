use rustc_hash::FxHashMap;

use super::ty::{GenericId, Type};

pub struct GenericBinding {
    bound: FxHashMap<GenericId, Type>,
}

impl GenericBinding {
    pub fn of(declared: &Type, used: &Type) -> Self {
        Self::of_each([declared], [used])
    }

    pub fn of_each<'t>(
        declared: impl IntoIterator<Item = &'t Type>,
        used: impl IntoIterator<Item = &'t Type>,
    ) -> Self {
        Self {
            bound: BindingWalk::of_each(declared, used).finish(),
        }
    }

    pub fn substitute(&self, declared: &[Type]) -> Vec<Type> {
        declared
            .iter()
            .map(|parameter| parameter.substitute(&self.bound))
            .collect()
    }

    pub fn into_bound(self) -> FxHashMap<GenericId, Type> {
        self.bound
    }
}

struct BindingWalk {
    exact: FxHashMap<GenericId, Type>,
    through_reference: FxHashMap<GenericId, Type>,
}

impl BindingWalk {
    fn of(declared: &Type, used: &Type) -> Self {
        Self::of_each([declared], [used])
    }

    fn of_each<'t>(
        declared: impl IntoIterator<Item = &'t Type>,
        used: impl IntoIterator<Item = &'t Type>,
    ) -> Self {
        let mut walk = Self {
            exact: FxHashMap::default(),
            through_reference: FxHashMap::default(),
        };
        walk.bind_each(declared, used);
        walk
    }

    fn bind(&mut self, declared: &Type, used: &Type) {
        match declared {
            Type::Generic(generic) => {
                self.exact.entry(*generic).or_insert_with(|| used.clone());
            }
            Type::Ref(_, inner) | Type::Mut(_, inner) => {
                self.absorb_behind_reference(Self::of(inner, used.referent()));
            }
            _ => self.bind_parts(declared, used),
        }
    }

    fn absorb_behind_reference(&mut self, binding: Self) {
        for (generic, used) in binding.finish() {
            self.through_reference.entry(generic).or_insert(used);
        }
    }

    fn finish(self) -> FxHashMap<GenericId, Type> {
        let mut bound = self.through_reference;
        bound.extend(self.exact);
        bound
    }

    fn bind_parts(&mut self, declared: &Type, used: &Type) {
        if declared.shares_head_with(used) {
            self.bind_each(declared.parts(), used.parts());
        }
    }

    fn bind_each<'t>(
        &mut self,
        declared: impl IntoIterator<Item = &'t Type>,
        used: impl IntoIterator<Item = &'t Type>,
    ) {
        for (declared, used) in declared.into_iter().zip(used) {
            self.bind(declared, used);
        }
    }
}
