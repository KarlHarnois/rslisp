use std::collections::HashMap;

use super::ty::{GenericId, Type};

pub struct GenericBinding {
    bound: HashMap<GenericId, Type>,
}

impl GenericBinding {
    pub fn of(declared: &Type, used: &Type) -> Self {
        Self::of_each([declared], [used])
    }

    pub fn of_each<'t>(
        declared: impl IntoIterator<Item = &'t Type>,
        used: impl IntoIterator<Item = &'t Type>,
    ) -> Self {
        let mut binding = Self {
            bound: HashMap::new(),
        };
        binding.bind_each(declared, used);
        binding
    }

    pub fn substitute(&self, declared: &[Type]) -> Vec<Type> {
        declared
            .iter()
            .map(|parameter| parameter.substitute(&self.bound))
            .collect()
    }

    pub fn into_bound(self) -> HashMap<GenericId, Type> {
        self.bound
    }

    fn bind(&mut self, declared: &Type, used: &Type) {
        match declared {
            Type::Generic(generic) => {
                self.bound.insert(*generic, used.clone());
            }
            Type::Ref(_, inner) | Type::Mut(_, inner) => self.bind(inner, Self::referent(used)),
            _ => self.bind_each(declared.parts(), used.parts()),
        }
    }

    fn referent(used: &Type) -> &Type {
        match used {
            Type::Ref(_, inner) | Type::Mut(_, inner) => inner,
            other => other,
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
