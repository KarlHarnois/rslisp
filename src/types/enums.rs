use std::collections::HashMap;
use std::rc::Rc;

use super::ty::{GenericId, GenericParam, Type};
use crate::core::BindingId;
use crate::source::Span;

pub enum VariantPayload {
    None,
    Positional(Vec<Type>),
    Named(Vec<(Rc<str>, Type)>),
}

pub struct VariantDefinition {
    pub name: Rc<str>,
    pub payload: VariantPayload,
    pub span: Span,
}

impl VariantDefinition {
    pub fn payload_types(&self) -> Vec<&Type> {
        match &self.payload {
            VariantPayload::None => Vec::new(),
            VariantPayload::Positional(types) => types.iter().collect(),
            VariantPayload::Named(fields) => {
                fields.iter().map(|(_, field_type)| field_type).collect()
            }
        }
    }

    pub fn instantiated_payload(&self, replacements: &HashMap<GenericId, Type>) -> Vec<Type> {
        match &self.payload {
            VariantPayload::None => Vec::new(),
            VariantPayload::Positional(types) => types
                .iter()
                .map(|payload| payload.substitute(replacements))
                .collect(),
            VariantPayload::Named(fields) => fields
                .iter()
                .map(|(_, payload)| payload.substitute(replacements))
                .collect(),
        }
    }
}

pub struct EnumDefinition {
    pub params: Vec<GenericParam>,
    pub variants: Vec<VariantDefinition>,
    pub derives: Vec<&'static str>,
    pub derived_traits: Vec<BindingId>,
    pub blocked: Vec<(BindingId, Rc<str>, Type)>,
    pub written_derive: Option<Span>,
}

impl EnumDefinition {
    pub fn instantiated_payload(&self, index: usize, arguments: &[Type]) -> Option<Vec<Type>> {
        let replacements = self.replacements(arguments);
        self.variants
            .get(index)
            .map(|variant| variant.instantiated_payload(&replacements))
    }

    pub fn payload_types(&self) -> impl Iterator<Item = &Type> {
        self.variants
            .iter()
            .flat_map(|variant| variant.payload_types())
    }

    fn replacements(&self, arguments: &[Type]) -> HashMap<GenericId, Type> {
        self.params
            .iter()
            .map(|param| param.id)
            .zip(arguments.iter().cloned())
            .collect()
    }
}

#[derive(Default)]
pub struct EnumTable {
    enums: HashMap<BindingId, EnumDefinition>,
}

impl EnumTable {
    pub fn add(&mut self, binding: BindingId, definition: EnumDefinition) {
        self.enums.insert(binding, definition);
    }

    pub fn get(&self, binding: BindingId) -> Option<&EnumDefinition> {
        self.enums.get(&binding)
    }
}

#[cfg(test)]
mod tests {
    use super::{EnumDefinition, VariantDefinition, VariantPayload};
    use crate::core::PrimitiveType;
    use crate::source::{SourceId, Span};
    use crate::types::ty::{GenericId, GenericParam, Type};

    struct Enums {
        span: Span,
    }

    impl Enums {
        fn new() -> Self {
            Self {
                span: Span::new(SourceId::new(0), 0..0),
            }
        }

        fn variant(&self, name: &str, payload: VariantPayload) -> VariantDefinition {
            VariantDefinition {
                name: name.into(),
                payload,
                span: self.span,
            }
        }

        fn definition(&self, variants: Vec<VariantDefinition>) -> EnumDefinition {
            EnumDefinition {
                params: Vec::new(),
                variants,
                derives: Vec::new(),
                derived_traits: Vec::new(),
                blocked: Vec::new(),
                written_derive: None,
            }
        }
    }

    #[test]
    fn a_generic_payload_substitutes_its_argument() {
        let fixture = Enums::new();
        let parameter = GenericParam {
            id: GenericId::new(0),
            name: "a".into(),
            bounds: Vec::new(),
        };
        let definition = EnumDefinition {
            params: vec![parameter.clone()],
            variants: vec![fixture.variant(
                "value",
                VariantPayload::Positional(vec![Type::Generic(parameter.id)]),
            )],
            derives: Vec::new(),
            derived_traits: Vec::new(),
            blocked: Vec::new(),
            written_derive: None,
        };
        assert_eq!(
            definition.instantiated_payload(0, &[Type::Primitive(PrimitiveType::I64)]),
            Some(vec![Type::Primitive(PrimitiveType::I64)])
        );
    }

    #[test]
    fn a_named_payload_keeps_its_field_order() {
        let fixture = Enums::new();
        let definition = fixture.definition(vec![fixture.variant(
            "labeled",
            VariantPayload::Named(vec![
                ("text".into(), Type::Primitive(PrimitiveType::String)),
                ("size".into(), Type::Primitive(PrimitiveType::F64)),
            ]),
        )]);
        assert_eq!(
            definition.instantiated_payload(0, &[]),
            Some(vec![
                Type::Primitive(PrimitiveType::String),
                Type::Primitive(PrimitiveType::F64),
            ])
        );
        assert_eq!(definition.payload_types().count(), 2);
    }

    #[test]
    fn a_variant_without_a_payload_has_no_payload_types() {
        let fixture = Enums::new();
        let definition = fixture.definition(vec![fixture.variant("empty", VariantPayload::None)]);
        assert_eq!(definition.instantiated_payload(0, &[]), Some(Vec::new()));
        assert_eq!(definition.payload_types().count(), 0);
    }
}
