use crate::types::{Type, TypedProgram};

#[derive(Clone)]
pub(super) struct Store {
    pub target: usize,
    pub value: usize,
}

pub(super) struct StoredArguments<'t> {
    parameters: &'t [Type],
}

impl<'t> StoredArguments<'t> {
    pub(super) fn of(callee: &'t Type) -> Self {
        let parameters = match callee {
            Type::Function(_, parameters, _) => parameters.as_slice(),
            _ => &[],
        };
        Self { parameters }
    }

    pub(super) fn stores(&self, program: &TypedProgram) -> Vec<Store> {
        self.parameters
            .iter()
            .enumerate()
            .flat_map(|(target, parameter)| self.stores_into(program, target, parameter))
            .collect()
    }

    fn stores_into(&self, program: &TypedProgram, target: usize, parameter: &Type) -> Vec<Store> {
        let Type::Mut(_, referent) = parameter else {
            return Vec::new();
        };
        if !program.type_holds_borrow(referent) {
            return Vec::new();
        }
        self.parameters
            .iter()
            .enumerate()
            .filter(|(value, stored)| *value != target && referent.contains(stored))
            .map(|(value, _)| Store { target, value })
            .collect()
    }
}
