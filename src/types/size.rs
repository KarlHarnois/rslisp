use std::mem::{align_of, size_of};

use super::enums::EnumTable;
use super::structs::StructTable;
use super::ty::Type;
use crate::core::{BindingId, PrimitiveType};

pub struct TypeSizes<'a> {
    structs: &'a StructTable,
    enums: &'a EnumTable,
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

    pub fn new(structs: &'a StructTable, enums: &'a EnumTable) -> Self {
        Self { structs, enums }
    }

    pub fn is_small(&self, ty: &Type) -> bool {
        self.layout(ty, &mut Vec::new())
            .is_none_or(|layout| layout.size <= Self::LIMIT)
    }

    fn layout(&self, ty: &Type, visiting: &mut Vec<BindingId>) -> Option<Layout> {
        match ty {
            Type::Primitive(primitive) => Self::primitive(*primitive),
            Type::Ref(_) | Type::Mut(_) => Some(Layout::POINTER),
            Type::Named(binding, arguments) => {
                if self.structs.get(*binding).is_some() {
                    self.structure(*binding, arguments, visiting)
                } else {
                    self.enumeration(*binding, arguments, visiting)
                }
            }
            Type::Var(_) | Type::Generic(_) | Type::Function(..) => None,
        }
    }

    fn primitive(primitive: PrimitiveType) -> Option<Layout> {
        Self::SIZED_PRIMITIVES
            .iter()
            .find(|(known, _)| *known == primitive)
            .map(|(_, layout)| *layout)
    }

    fn enumeration(
        &self,
        binding: BindingId,
        arguments: &[Type],
        visiting: &mut Vec<BindingId>,
    ) -> Option<Layout> {
        if visiting.contains(&binding) {
            return None;
        }
        let definition = self.enums.get(binding)?;
        if definition.rust.is_some() {
            return None;
        }
        visiting.push(binding);
        let layout = definition.variants.iter().enumerate().try_fold(
            Layout {
                size: 0,
                alignment: 1,
            },
            |largest, (index, _)| {
                let payload = definition.instantiated_payload(index, arguments)?;
                let variant = self.payload_layout(&payload, visiting)?;
                Some(Layout {
                    size: largest.size.max(variant.size),
                    alignment: largest.alignment.max(variant.alignment),
                })
            },
        );
        visiting.pop();
        layout.map(|layout| Layout {
            size: layout.size.saturating_add(8).max(1),
            alignment: layout.alignment.max(8),
        })
    }

    fn payload_layout(&self, payload: &[Type], visiting: &mut Vec<BindingId>) -> Option<Layout> {
        payload.iter().try_fold(
            Layout {
                size: 0,
                alignment: 1,
            },
            |layout, payload_type| {
                let part = self.layout(payload_type, visiting)?;
                Some(Layout {
                    size: layout.size + part.size,
                    alignment: layout.alignment.max(part.alignment),
                })
            },
        )
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

#[cfg(test)]
mod tests {
    use super::TypeSizes;
    use crate::core::{BindingArena, BindingInfo, BindingKind, ModuleId, PrimitiveType};
    use crate::types::Type;
    use crate::types::enums::{EnumDefinition, EnumTable, VariantDefinition, VariantPayload};
    use crate::types::structs::{StructDefinition, StructTable};

    struct Sizes {
        bindings: BindingArena,
        structs: StructTable,
        enums: EnumTable,
    }

    impl Sizes {
        fn new() -> Self {
            Self {
                bindings: BindingArena::default(),
                structs: StructTable::default(),
                enums: EnumTable::default(),
            }
        }

        fn type_name(&mut self, name: &str, kind: BindingKind) -> crate::core::BindingId {
            self.bindings.add(BindingInfo::new(name.into(), kind, None))
        }

        fn two_numbers(&mut self) -> Type {
            let binding = self.type_name("pair", BindingKind::Struct(ModuleId::new(0)));
            self.structs.add(
                binding,
                StructDefinition {
                    params: Vec::new(),
                    fields: vec![
                        ("first".into(), Type::Primitive(PrimitiveType::I64)),
                        ("second".into(), Type::Primitive(PrimitiveType::I64)),
                    ],
                    derives: Vec::new(),
                    derived_traits: Vec::new(),
                    blocked: Vec::new(),
                    written_derive: None,
                },
            );
            Type::Named(binding, Vec::new())
        }

        fn enumeration(&mut self, payload: VariantPayload) -> Type {
            let binding = self.type_name("shape", BindingKind::Enum(ModuleId::new(0)));
            self.enums.add(
                binding,
                EnumDefinition {
                    params: Vec::new(),
                    variants: vec![VariantDefinition {
                        name: "value".into(),
                        payload,
                        span: crate::source::Span::new(crate::source::SourceId::new(0), 0..0),
                    }],
                    rust: None,
                    derives: Vec::new(),
                    derived_traits: Vec::new(),
                    blocked: Vec::new(),
                    written_derive: None,
                },
            );
            Type::Named(binding, Vec::new())
        }

        fn recursive_struct(&mut self) -> Type {
            let binding = self.type_name("node", BindingKind::Struct(ModuleId::new(0)));
            self.structs.add(
                binding,
                StructDefinition {
                    params: Vec::new(),
                    fields: vec![("next".into(), Type::Named(binding, Vec::new()))],
                    derives: Vec::new(),
                    derived_traits: Vec::new(),
                    blocked: Vec::new(),
                    written_derive: None,
                },
            );
            Type::Named(binding, Vec::new())
        }

        fn recursive_enum(&mut self) -> Type {
            let binding = self.type_name("node", BindingKind::Enum(ModuleId::new(0)));
            self.enums.add(
                binding,
                EnumDefinition {
                    params: Vec::new(),
                    variants: vec![VariantDefinition {
                        name: "next".into(),
                        payload: VariantPayload::Positional(vec![Type::Named(binding, Vec::new())]),
                        span: crate::source::Span::new(crate::source::SourceId::new(0), 0..0),
                    }],
                    rust: None,
                    derives: Vec::new(),
                    derived_traits: Vec::new(),
                    blocked: Vec::new(),
                    written_derive: None,
                },
            );
            Type::Named(binding, Vec::new())
        }

        fn sizes(&self) -> TypeSizes<'_> {
            TypeSizes::new(&self.structs, &self.enums)
        }
    }

    #[test]
    fn a_layout_records_its_size_and_alignment() {
        let layout = super::Layout::of::<u64>();
        assert_eq!(layout.size, 8);
        assert_eq!(layout.alignment, 8);
    }

    #[test]
    fn an_enum_with_no_payload_is_small() {
        let mut fixture = Sizes::new();
        let value = fixture.enumeration(VariantPayload::None);
        assert!(fixture.sizes().is_small(&value));
    }

    #[test]
    fn an_enum_with_a_large_payload_is_not_small() {
        let mut fixture = Sizes::new();
        let pair = fixture.two_numbers();
        let value = fixture.enumeration(VariantPayload::Positional(vec![pair]));
        assert!(!fixture.sizes().is_small(&value));
    }

    #[test]
    fn a_recursive_struct_has_no_known_size() {
        let mut fixture = Sizes::new();
        let value = fixture.recursive_struct();
        assert!(fixture.sizes().is_small(&value));
    }

    #[test]
    fn a_recursive_enum_has_no_known_size() {
        let mut fixture = Sizes::new();
        let value = fixture.recursive_enum();
        assert!(fixture.sizes().is_small(&value));
    }
}
