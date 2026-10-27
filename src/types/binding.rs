use std::collections::HashMap;

use super::ty::{GenericId, Type};

pub struct GenericBinding {
    exact: HashMap<GenericId, Type>,
    through_reference: HashMap<GenericId, Type>,
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
            exact: HashMap::new(),
            through_reference: HashMap::new(),
        };
        binding.bind_each(declared, used);
        binding
    }

    pub fn substitute(&self, declared: &[Type]) -> Vec<Type> {
        let replacements = self.replacements();
        declared
            .iter()
            .map(|parameter| parameter.substitute(&replacements))
            .collect()
    }

    pub fn into_bound(self) -> HashMap<GenericId, Type> {
        Self::prefer_exact(self.through_reference, self.exact)
    }

    fn replacements(&self) -> HashMap<GenericId, Type> {
        Self::prefer_exact(self.through_reference.clone(), self.exact.clone())
    }

    fn prefer_exact(
        mut through_reference: HashMap<GenericId, Type>,
        exact: HashMap<GenericId, Type>,
    ) -> HashMap<GenericId, Type> {
        through_reference.extend(exact);
        through_reference
    }

    fn bind(&mut self, declared: &Type, used: &Type) {
        match declared {
            Type::Generic(generic) => {
                self.exact.entry(*generic).or_insert_with(|| used.clone());
            }
            Type::Ref(_, inner) | Type::Mut(_, inner) => {
                self.bind_through_reference(inner, Self::referent(used));
            }
            _ => self.bind_parts(declared, used),
        }
    }

    fn bind_through_reference(&mut self, declared: &Type, used: &Type) {
        match declared {
            Type::Generic(generic) => {
                self.through_reference
                    .entry(*generic)
                    .or_insert_with(|| used.clone());
            }
            Type::Ref(_, inner) | Type::Mut(_, inner) => {
                self.bind_through_reference(inner, Self::referent(used));
            }
            _ => self.bind_parts_through_reference(declared, used),
        }
    }

    fn bind_parts(&mut self, declared: &Type, used: &Type) {
        if declared.shares_head_with(used) {
            self.bind_each(declared.parts(), used.parts());
        }
    }

    fn bind_parts_through_reference(&mut self, declared: &Type, used: &Type) {
        if declared.shares_head_with(used) {
            self.bind_each_through_reference(declared.parts(), used.parts());
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

    fn bind_each_through_reference<'t>(
        &mut self,
        declared: impl IntoIterator<Item = &'t Type>,
        used: impl IntoIterator<Item = &'t Type>,
    ) {
        for (declared, used) in declared.into_iter().zip(used) {
            self.bind_through_reference(declared, used);
        }
    }
}
