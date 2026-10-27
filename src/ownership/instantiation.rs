use std::collections::HashMap;

use crate::types::{GenericId, Type};

pub struct UseInstantiation {
    bound: HashMap<GenericId, Type>,
}

impl UseInstantiation {
    pub fn of(declared: &[Type], used: &[Type]) -> Self {
        let mut instantiation = Self {
            bound: HashMap::new(),
        };
        instantiation.bind_parts(declared, used);
        instantiation
    }

    pub fn apply(&self, declared: &[Type]) -> Vec<Type> {
        declared
            .iter()
            .map(|parameter| parameter.substitute(&self.bound))
            .collect()
    }

    fn bind(&mut self, declared: &Type, used: &Type) {
        match declared {
            Type::Generic(generic) => {
                self.bound.entry(*generic).or_insert_with(|| used.clone());
            }
            Type::Ref(_, inner) | Type::Mut(_, inner) if !used.is_reference() => {
                self.bind(inner, used);
            }
            _ => self.bind_parts(declared.parts(), used.parts()),
        }
    }

    fn bind_parts<'t>(
        &mut self,
        declared: impl IntoIterator<Item = &'t Type>,
        used: impl IntoIterator<Item = &'t Type>,
    ) {
        for (declared, used) in declared.into_iter().zip(used) {
            self.bind(declared, used);
        }
    }
}
