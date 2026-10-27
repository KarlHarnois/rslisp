use crate::types::{Type, TypedProgram};

pub struct SignatureBorrows<'a> {
    program: &'a TypedProgram,
}

impl<'a> SignatureBorrows<'a> {
    pub fn new(program: &'a TypedProgram) -> Self {
        Self { program }
    }

    pub fn holds_borrow(&self, ty: &Type) -> bool {
        self.program.type_holds_borrow(ty) || self.is_borrowing_function(ty)
    }

    pub fn is_borrowing_function(&self, ty: &Type) -> bool {
        match ty {
            Type::Function(_, parameters, result) => {
                parameters
                    .iter()
                    .any(|parameter| self.holds_borrow(parameter))
                    || self.holds_borrow(result)
            }
            _ => false,
        }
    }
}
