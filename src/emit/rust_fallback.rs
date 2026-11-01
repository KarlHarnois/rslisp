use crate::core::PrimitiveType;
use crate::reader::{Number, NumberValue};
use crate::types::Type;

pub(super) struct RustFallback;

impl RustFallback {
    pub(super) const INTEGER: PrimitiveType = PrimitiveType::I32;
    const FLOAT: PrimitiveType = PrimitiveType::F64;

    pub(super) fn gives(number: &Number, ty: &Type) -> bool {
        *ty == Type::Primitive(Self::of(number, ty))
    }

    fn of(number: &Number, ty: &Type) -> PrimitiveType {
        let float_digits = matches!(number.value(), NumberValue::Float(_))
            || matches!(ty, Type::Primitive(primitive) if primitive.is_float());
        if float_digits {
            Self::FLOAT
        } else {
            Self::INTEGER
        }
    }
}
