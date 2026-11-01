use super::rust_fallback::RustFallback;
use crate::types::{Type, TypedBinder, TypedProgram};

pub(super) struct LetAnnotation<'a> {
    program: &'a TypedProgram,
}

impl<'a> LetAnnotation<'a> {
    pub(super) fn new(program: &'a TypedProgram) -> Self {
        Self { program }
    }

    pub(super) fn is_emitted(&self, binder: &TypedBinder) -> bool {
        binder.annotated || self.is_needed(&binder.ty)
    }

    fn is_needed(&self, bound: &Type) -> bool {
        match bound {
            Type::Primitive(primitive) => {
                primitive.is_integer() && *primitive != RustFallback::INTEGER
            }
            Type::Named(binding, arguments) => {
                !self.program.rust_types.is_opaque(*binding)
                    && arguments.iter().any(|argument| self.is_needed(argument))
            }
            _ => false,
        }
    }
}
