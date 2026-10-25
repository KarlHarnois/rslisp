use std::collections::HashMap;
use std::rc::Rc;

use super::ty::{GenericId, GenericParam, Type};
use crate::core::BindingId;
use crate::source::Span;

#[derive(Clone)]
pub enum VariantPayload {
    None,
    Positional(Vec<Type>),
    Named(Vec<(Rc<str>, Type)>),
}

#[derive(Clone)]
pub struct VariantDefinition {
    pub name: Rc<str>,
    pub payload: VariantPayload,
    pub payload_spans: Vec<Span>,
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
                .map(|payload| payload.without_written_lifetimes().substitute(replacements))
                .collect(),
            VariantPayload::Named(fields) => fields
                .iter()
                .map(|(_, payload)| payload.without_written_lifetimes().substitute(replacements))
                .collect(),
        }
    }
}

#[derive(Clone)]
pub struct EnumDefinition {
    pub lifetimes: Vec<BindingId>,
    pub params: Vec<GenericParam>,
    pub variants: Vec<VariantDefinition>,
    pub rust: Option<crate::core::EnumRust>,
    pub derives: Vec<&'static str>,
    pub derived_traits: Vec<BindingId>,
    pub blocked: Vec<(BindingId, Rc<str>, Type)>,
    pub written_derive: Option<Span>,
}

impl EnumDefinition {
    pub fn is_bare_rust(&self) -> bool {
        self.rust
            .as_ref()
            .is_some_and(|rust| rust.bare && !rust.template_clause)
    }

    pub fn is_result_shaped(&self) -> bool {
        self.is_bare_rust()
            && self.params.len() == 2
            && self.variants.len() == 2
            && self.holds_parameter(0, 0)
            && self.holds_parameter(1, 1)
    }

    pub fn is_option_shaped(&self) -> bool {
        self.is_bare_rust()
            && self.params.len() == 1
            && self.variants.len() == 2
            && self.holds_parameter(0, 0)
            && matches!(
                self.variants.get(1).map(|variant| &variant.payload),
                Some(VariantPayload::None)
            )
    }

    pub fn holds_parameter(&self, variant: usize, parameter: usize) -> bool {
        let Some(parameter) = self.params.get(parameter) else {
            return false;
        };
        matches!(
            self.variants.get(variant).map(|variant| &variant.payload),
            Some(VariantPayload::Positional(parts))
                if matches!(parts.as_slice(), [Type::Generic(id)] if *id == parameter.id)
        )
    }

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

    pub fn blocking_variant(&self, satisfies: &dyn Fn(&Type) -> bool) -> Option<(Rc<str>, Type)> {
        self.variants.iter().find_map(|variant| {
            variant
                .payload_types()
                .into_iter()
                .find(|payload| !satisfies(payload))
                .map(|payload| (variant.name.clone(), payload.clone()))
        })
    }

    fn replacements(&self, arguments: &[Type]) -> HashMap<GenericId, Type> {
        self.params
            .iter()
            .map(|param| param.id)
            .zip(arguments.iter().cloned())
            .collect()
    }
}

#[derive(Clone, Default)]
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

    pub fn get_mut(&mut self, binding: BindingId) -> Option<&mut EnumDefinition> {
        self.enums.get_mut(&binding)
    }

    pub fn iter(&self) -> impl Iterator<Item = (BindingId, &EnumDefinition)> {
        self.enums
            .iter()
            .map(|(binding, definition)| (*binding, definition))
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
                payload_spans: Vec::new(),
                span: self.span,
            }
        }

        fn definition(&self, variants: Vec<VariantDefinition>) -> EnumDefinition {
            EnumDefinition {
                lifetimes: Vec::new(),
                params: Vec::new(),
                variants,
                rust: None,
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
            lifetimes: Vec::new(),
            params: vec![parameter.clone()],
            variants: vec![fixture.variant(
                "value",
                VariantPayload::Positional(vec![Type::Generic(parameter.id)]),
            )],
            rust: None,
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

    #[test]
    fn a_blocking_variant_names_the_payload_that_fails() {
        let fixture = Enums::new();
        let definition = fixture.definition(vec![
            fixture.variant(
                "text",
                VariantPayload::Positional(vec![Type::Primitive(PrimitiveType::String)]),
            ),
            fixture.variant(
                "number",
                VariantPayload::Positional(vec![Type::Primitive(PrimitiveType::F64)]),
            ),
        ]);
        let blocking = definition
            .blocking_variant(&|payload| matches!(payload, Type::Primitive(PrimitiveType::String)))
            .expect("the number payload does not satisfy the test");
        assert_eq!(blocking.0.as_ref(), "number");
        assert_eq!(blocking.1, Type::Primitive(PrimitiveType::F64));
    }
}
