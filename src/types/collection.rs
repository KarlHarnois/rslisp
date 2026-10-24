use super::rust_types::RustTypeTable;
use super::ty::Type;
use crate::core::PrimitiveType;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CollectionKind {
    Vector,
    SortedMap,
}

pub struct Collection {
    kind: CollectionKind,
    ty: Type,
    key: Type,
    value: Type,
}

impl Collection {
    pub fn of(ty: &Type, rust_types: &RustTypeTable) -> Option<Self> {
        let Type::Named(binding, arguments) = ty else {
            return None;
        };
        if rust_types.is_vector(*binding) {
            let [value] = arguments.as_slice() else {
                return None;
            };
            return Some(Self {
                kind: CollectionKind::Vector,
                ty: ty.clone(),
                key: Type::Primitive(PrimitiveType::Usize),
                value: value.clone(),
            });
        }
        if rust_types.is_sorted_map(*binding) {
            let [key, value] = arguments.as_slice() else {
                return None;
            };
            return Some(Self {
                kind: CollectionKind::SortedMap,
                ty: ty.clone(),
                key: key.clone(),
                value: value.clone(),
            });
        }
        None
    }

    pub fn kind(&self) -> CollectionKind {
        self.kind
    }

    pub fn ty(&self) -> &Type {
        &self.ty
    }

    pub fn key(&self) -> &Type {
        &self.key
    }

    pub fn value(&self) -> &Type {
        &self.value
    }

    pub fn is_map(&self) -> bool {
        self.kind == CollectionKind::SortedMap
    }
}
