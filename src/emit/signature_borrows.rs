use crate::types::{Type, TypedProgram};

pub struct SignatureBorrows<'a> {
    program: &'a TypedProgram,
}

impl<'a> SignatureBorrows<'a> {
    pub fn new(program: &'a TypedProgram) -> Self {
        Self { program }
    }

    pub fn holds_borrow(&self, ty: &Type) -> bool {
        self.program.type_holds_borrow(ty)
    }

    pub fn is_borrowing_function(&self, ty: &Type) -> bool {
        match ty {
            Type::Function(_, parameters, result) => parameters
                .iter()
                .chain(std::iter::once(result.as_ref()))
                .any(|part| self.holds_borrow(part) || self.is_borrowing_function(part)),
            _ => false,
        }
    }
}
