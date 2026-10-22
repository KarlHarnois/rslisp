use std::mem::{align_of, size_of};

use super::structs::StructTable;
use super::ty::Type;
use crate::core::{BindingId, PrimitiveType};

pub struct TypeSizes<'a> {
    structs: &'a StructTable,
}

#[derive(Clone, Copy)]
struct Layout {
    size: usize,
    alignment: usize,
}

impl Layout {
    const POINTER: Layout = Layout::of::<usize>();

    const fn of<T>() -> Layout {
        Layout {
            size: size_of::<T>(),
            alignment: align_of::<T>(),
        }
    }
}

impl<'a> TypeSizes<'a> {
    const LIMIT: usize = 16;
    const SIZED_PRIMITIVES: [(PrimitiveType, Layout); 18] = [
        (PrimitiveType::Unit, Layout::of::<()>()),
        (PrimitiveType::Bool, Layout::of::<bool>()),
        (PrimitiveType::Char, Layout::of::<char>()),
        (PrimitiveType::I8, Layout::of::<i8>()),
        (PrimitiveType::I16, Layout::of::<i16>()),
        (PrimitiveType::I32, Layout::of::<i32>()),
        (PrimitiveType::I64, Layout::of::<i64>()),
        (PrimitiveType::I128, Layout::of::<i128>()),
        (PrimitiveType::Isize, Layout::of::<isize>()),
        (PrimitiveType::U8, Layout::of::<u8>()),
        (PrimitiveType::U16, Layout::of::<u16>()),
        (PrimitiveType::U32, Layout::of::<u32>()),
        (PrimitiveType::U64, Layout::of::<u64>()),
        (PrimitiveType::U128, Layout::of::<u128>()),
        (PrimitiveType::Usize, Layout::of::<usize>()),
        (PrimitiveType::F32, Layout::of::<f32>()),
        (PrimitiveType::F64, Layout::of::<f64>()),
        (PrimitiveType::String, Layout::of::<String>()),
    ];

    pub fn new(structs: &'a StructTable) -> Self {
        Self { structs }
    }

    pub fn is_small(&self, ty: &Type) -> bool {
        self.layout(ty, &mut Vec::new())
            .is_none_or(|layout| layout.size <= Self::LIMIT)
    }

    fn layout(&self, ty: &Type, visiting: &mut Vec<BindingId>) -> Option<Layout> {
        match ty {
            Type::Primitive(primitive) => Self::primitive(*primitive),
            Type::Ref(_) | Type::Mut(_) => Some(Layout::POINTER),
            Type::Named(binding, arguments) => self.structure(*binding, arguments, visiting),
            Type::Var(_) | Type::Generic(_) | Type::Function(..) => None,
        }
    }

    fn primitive(primitive: PrimitiveType) -> Option<Layout> {
        Self::SIZED_PRIMITIVES
            .iter()
            .find(|(known, _)| *known == primitive)
            .map(|(_, layout)| *layout)
    }

    fn structure(
        &self,
        binding: BindingId,
        arguments: &[Type],
        visiting: &mut Vec<BindingId>,
    ) -> Option<Layout> {
        if visiting.contains(&binding) {
            return None;
        }
        let definition = self.structs.get(binding)?;
        let fields = definition.instantiated_fields(arguments);
        visiting.push(binding);
        let mut size = 0;
        let mut alignment = 1;
        for (_, field) in &fields {
            let layout = self.layout(field, visiting)?;
            size += layout.size;
            alignment = alignment.max(layout.alignment);
        }
        visiting.pop();
        Some(Layout {
            size: size.div_ceil(alignment) * alignment,
            alignment,
        })
    }
}
