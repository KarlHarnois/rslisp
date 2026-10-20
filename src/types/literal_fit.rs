use super::ty::Type;
use crate::core::PrimitiveType;
use crate::reader::NumberValue;

pub struct LiteralFit;

impl LiteralFit {
    pub fn fits(value: &NumberValue, candidate: &Type) -> bool {
        let Type::Primitive(primitive) = candidate else {
            return false;
        };
        match value {
            NumberValue::Float(_) => primitive.is_float(),
            NumberValue::Integer(_) if primitive.is_float() => true,
            NumberValue::Integer(integer) => Self::integer_range(*primitive)
                .is_some_and(|(minimum, maximum)| minimum <= *integer && *integer <= maximum),
        }
    }

    fn integer_range(primitive: PrimitiveType) -> Option<(i128, i128)> {
        Some(match primitive {
            PrimitiveType::I8 => (i8::MIN.into(), i8::MAX.into()),
            PrimitiveType::I16 => (i16::MIN.into(), i16::MAX.into()),
            PrimitiveType::I32 => (i32::MIN.into(), i32::MAX.into()),
            PrimitiveType::I64 | PrimitiveType::Isize => (i64::MIN.into(), i64::MAX.into()),
            PrimitiveType::I128 => (i128::MIN, i128::MAX),
            PrimitiveType::U8 => (0, u8::MAX.into()),
            PrimitiveType::U16 => (0, u16::MAX.into()),
            PrimitiveType::U32 => (0, u32::MAX.into()),
            PrimitiveType::U64 | PrimitiveType::Usize => (0, u64::MAX.into()),
            PrimitiveType::U128 => (0, i128::MAX),
            _ => return None,
        })
    }
}
