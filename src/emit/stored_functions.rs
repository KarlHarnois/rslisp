use crate::core::BindingId;
use crate::types::TypedProgram;

pub struct StoredFunctions<'a> {
    program: &'a TypedProgram,
}

impl<'a> StoredFunctions<'a> {
    pub fn new(program: &'a TypedProgram) -> Self {
        Self { program }
    }

    pub fn stores(&self, binding: BindingId) -> bool {
        self.program.stored_function_parameters.contains(&binding)
    }
}
