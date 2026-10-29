use super::opaque_borrows::OpaqueBorrows;
use crate::types::{Type, TypedProgram};

pub struct SignatureBorrows<'a> {
    program: &'a TypedProgram,
}

impl<'a> SignatureBorrows<'a> {
    pub fn new(program: &'a TypedProgram) -> Self {
        Self { program }
    }

    pub fn hides_borrow_from_elision(&self, ty: &Type) -> bool {
        self.is_borrowing_function(ty) || OpaqueBorrows::new(self.program).holds_borrow(ty)
    }

    pub fn is_borrowing_function(&self, ty: &Type) -> bool {
        match ty {
            Type::Function(_, parameters, result) => parameters
                .iter()
                .chain(std::iter::once(result.as_ref()))
                .any(|part| {
                    self.program.type_holds_borrow(part) || self.is_borrowing_function(part)
                }),
            _ => false,
        }
    }

    pub fn returns_borrow_from_no_argument(&self, ty: &Type) -> bool {
        match ty {
            Type::Function(_, parameters, result) => {
                self.program.function_type_returns_borrow(result)
                    && !parameters
                        .iter()
                        .any(|parameter| self.program.type_holds_borrow(parameter))
            }
            _ => false,
        }
    }
}
