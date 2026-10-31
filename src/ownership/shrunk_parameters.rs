use std::collections::HashMap;

use crate::core::BindingId;
use crate::types::{Type, TypedProgram};

#[derive(Default)]
pub struct ShrunkParameters {
    parameters: HashMap<BindingId, Type>,
    declarations: HashMap<BindingId, Vec<(usize, Type)>>,
}

impl ShrunkParameters {
    pub fn new(
        parameters: HashMap<BindingId, Type>,
        declarations: HashMap<BindingId, Vec<(usize, Type)>>,
    ) -> Self {
        Self {
            parameters,
            declarations,
        }
    }

    pub fn parameters(&self) -> &HashMap<BindingId, Type> {
        &self.parameters
    }

    pub fn rewrite_declarations(&self, program: &mut TypedProgram) {
        for definition in program
            .modules
            .iter_mut()
            .flat_map(|module| module.definitions.iter_mut())
        {
            for (position, shrunk) in self
                .declarations
                .get(&definition.binding)
                .into_iter()
                .flatten()
            {
                Self::replace_parameter(&mut definition.scheme.body, *position, shrunk);
            }
        }
    }

    fn replace_parameter(function: &mut Type, position: usize, shrunk: &Type) {
        if let Type::Function(_, parameters, _) = function
            && let Some(parameter) = parameters.get_mut(position)
        {
            *parameter = shrunk.clone();
        }
    }
}
