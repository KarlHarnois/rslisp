use super::rust_types::RustTypeTable;
use super::ty::Type;

#[derive(Clone, Copy)]
pub struct Pipelines<'a> {
    rust_types: &'a RustTypeTable,
}

impl<'a> Pipelines<'a> {
    pub fn new(rust_types: &'a RustTypeTable) -> Self {
        Self { rust_types }
    }

    pub fn is_pipeline(&self, ty: &Type) -> bool {
        matches!(ty, Type::Named(binding, _) if self.rust_types.is_iterator(*binding))
    }
}
