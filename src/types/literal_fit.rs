use super::ty::Type;
use crate::reader::NumberValue;

pub struct LiteralFit;

impl LiteralFit {
    pub fn is_representable(value: &NumberValue) -> bool {
        match value {
            NumberValue::Float(float) => float.is_finite(),
            NumberValue::Integer(_) => true,
        }
    }

    pub fn fits(value: &NumberValue, candidate: &Type) -> bool {
        let Type::Primitive(primitive) = candidate else {
            return false;
        };
        match value {
            NumberValue::Float(float) => primitive.is_float() && float.is_finite(),
            NumberValue::Integer(_) if primitive.is_float() => true,
            NumberValue::Integer(integer) => primitive
                .integer_range()
                .is_some_and(|(minimum, maximum)| minimum <= *integer && *integer <= maximum),
        }
    }
}
