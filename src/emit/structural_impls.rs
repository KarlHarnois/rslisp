use std::collections::BTreeSet;
use std::rc::Rc;

use super::fragment::Fragment;
use super::items::ItemEmitter;
use super::mangle::NameMangler;
use super::plan::EmitPlan;
use super::types::{LifetimeScope, RustTypes, TypePosition};
use super::unsized_parameters::UnsizedParameters;
use crate::core::{BindingArena, BindingId, ModuleId, StructuralKey, StructuralValues};
use crate::types::{
    Bound, GenericParam, StructuralTrait, StructuralUse, Type, TypedProgram, VariantDefinition,
    VariantPayload,
};

#[derive(Default)]
struct TemplateValues {
    parameters: Vec<String>,
    name: String,
    parts: String,
    field: String,
    value: String,
}

impl StructuralValues for TemplateValues {
    fn parameter(&self, index: usize) -> String {
        self.parameters[index].clone()
    }

    fn name(&self) -> String {
        self.name.clone()
    }

    fn parts(&self) -> String {
        self.parts.clone()
    }

    fn field(&self) -> String {
        self.field.clone()
    }

    fn value(&self) -> String {
        self.value.clone()
    }
}

struct ImplementedType<'t> {
    binding: BindingId,
    module: ModuleId,
    rust_name: &'t str,
    params: &'t [GenericParam],
}

struct ParameterNames<'b> {
    bindings: &'b BindingArena,
}

impl<'b> ParameterNames<'b> {
    const UNNAMED: &'static str = "parameter";

    fn new(bindings: &'b BindingArena) -> Self {
        Self { bindings }
    }

    fn of(&self, parameters: &[Type]) -> Vec<String> {
        let numbered = parameters.len() > 1;
        parameters
            .iter()
            .enumerate()
            .map(|(position, parameter)| {
                let name = self.type_name(parameter.referent());
                if numbered {
                    format!("{name}_{}", position + 1)
                } else {
                    name
                }
            })
            .collect()
    }

    fn type_name(&self, parameter: &Type) -> String {
        match parameter {
            Type::Named(binding, _) => NameMangler::value(self.bindings.get(*binding).name()),
            Type::Primitive(primitive) => primitive.name().to_string(),
            _ => Self::UNNAMED.to_string(),
        }
    }
}

pub struct StructuralImpls<'a> {
    program: &'a TypedProgram,
    plan: &'a EmitPlan<'a>,
    unsized_parameters: &'a UnsizedParameters,
    needed: BTreeSet<StructuralUse>,
}

impl<'a> StructuralImpls<'a> {
    const RECEIVER: &'static str = "self";
    const PART_PREFIX: &'static str = "part_";
    const QUALIFIER: &'static str = "/";

    pub fn new(
        program: &'a TypedProgram,
        plan: &'a EmitPlan<'a>,
        unsized_parameters: &'a UnsizedParameters,
    ) -> Self {
        let mut impls = Self {
            program,
            plan,
            unsized_parameters,
            needed: BTreeSet::new(),
        };
        impls.needed = impls.reached_uses();
        impls
    }

    fn reached_uses(&self) -> BTreeSet<StructuralUse> {
        let mut needed: BTreeSet<StructuralUse> = self
            .program
            .structural_uses
            .of_definitions(self.plan.reached())
            .into_iter()
            .filter(|used| self.derives(*used))
            .collect();
        let mut unchecked: Vec<StructuralUse> = needed.iter().copied().collect();
        while let Some(used) = unchecked.pop() {
            for head in self.payload_heads(used.head) {
                let next = StructuralUse {
                    trait_binding: used.trait_binding,
                    head,
                };
                if self.derives(next) && needed.insert(next) {
                    unchecked.push(next);
                }
            }
        }
        needed
    }

    fn derives(&self, used: StructuralUse) -> bool {
        self.derived_traits(used.head).contains(&used.trait_binding)
    }

    fn derived_traits(&self, head: BindingId) -> &'a [BindingId] {
        if let Some(definition) = self.program.structs.get(head) {
            return &definition.derived_traits;
        }
        self.program
            .enums
            .get(head)
            .map_or(&[], |definition| &definition.derived_traits)
    }

    fn payload_heads(&self, binding: BindingId) -> BTreeSet<BindingId> {
        let mut heads = BTreeSet::new();
        for payload in self.payload_types(binding) {
            Self::collect_heads(payload, &mut heads);
        }
        heads
    }

    fn payload_types(&self, binding: BindingId) -> Vec<&'a Type> {
        if let Some(definition) = self.program.structs.get(binding) {
            return definition.fields.iter().map(|(_, ty)| ty).collect();
        }
        self.program
            .enums
            .get(binding)
            .into_iter()
            .flat_map(|definition| &definition.variants)
            .flat_map(VariantDefinition::payload_types)
            .collect()
    }

    fn collect_heads(ty: &Type, heads: &mut BTreeSet<BindingId>) {
        if let Type::Named(binding, _) = ty {
            heads.insert(*binding);
        }
        for part in ty.parts() {
            Self::collect_heads(part, heads);
        }
    }

    pub fn of_module(&self, module: ModuleId) -> Vec<Vec<Fragment>> {
        let mut items = Vec::new();
        for binding in self.plan.items_of(module) {
            let info = self
                .plan
                .item(binding)
                .expect("each item of a module has its info");
            let implemented = ImplementedType {
                binding,
                module,
                rust_name: &info.name,
                params: self.type_params(binding),
            };
            items.extend(
                self.program
                    .structural_traits
                    .iter()
                    .filter(|structural| self.needs(structural.binding, binding))
                    .map(|structural| self.implementation(structural, &implemented)),
            );
        }
        items
    }

    fn needs(&self, trait_binding: BindingId, head: BindingId) -> bool {
        self.needed.contains(&StructuralUse {
            trait_binding,
            head,
        })
    }

    fn type_params(&self, binding: BindingId) -> &'a [GenericParam] {
        match self.program.structs.get(binding) {
            Some(definition) => &definition.params,
            None => self
                .program
                .enums
                .get(binding)
                .map_or(&[], |definition| &definition.params),
        }
    }

    fn implementation(
        &self,
        structural: &StructuralTrait,
        implemented: &ImplementedType,
    ) -> Vec<Fragment> {
        let types = RustTypes::new(
            self.program,
            self.plan,
            self.unsized_parameters,
            implemented.module,
        );
        let trait_path = self.plan.trait_path(structural.binding, implemented.module);
        vec![
            Fragment::Text(format!(
                "impl{} {trait_path} for {} {{",
                self.impl_generics(structural, implemented, &types),
                self.implementor_name(implemented, &types),
            )),
            Fragment::Indented(self.method(structural, implemented, &types)),
            Fragment::Newline,
            Fragment::Text("}".to_string()),
        ]
    }

    fn implementor(implemented: &ImplementedType) -> Type {
        let arguments = implemented
            .params
            .iter()
            .map(|param| Type::Generic(param.id))
            .collect();
        Type::Named(implemented.binding, arguments)
    }

    fn implementor_scope(&self, implemented: &ImplementedType) -> LifetimeScope {
        LifetimeScope::default().with_elided(self.implementor_lifetime(implemented))
    }

    fn implementor_lifetime(&self, implemented: &ImplementedType) -> Option<String> {
        ItemEmitter::implementor_lifetime_of(self.program, &Self::implementor(implemented))
    }

    fn implementor_name(&self, implemented: &ImplementedType, types: &RustTypes) -> String {
        types.show_in(
            &Self::implementor(implemented),
            TypePosition::Signature,
            &self.implementor_scope(implemented),
        )
    }

    fn impl_generics(
        &self,
        structural: &StructuralTrait,
        implemented: &ImplementedType,
        types: &RustTypes,
    ) -> String {
        let bounded: Vec<GenericParam> = implemented
            .params
            .iter()
            .map(|param| GenericParam {
                bounds: vec![Bound::plain(structural.binding)],
                ..param.clone()
            })
            .collect();
        let declared = types.generics(
            &bounded,
            &Self::implementor(implemented),
            &Default::default(),
        );
        let lifetimes: Vec<String> = self.implementor_lifetime(implemented).into_iter().collect();
        ItemEmitter::with_lifetimes(&lifetimes, declared)
    }

    fn method(
        &self,
        structural: &StructuralTrait,
        implemented: &ImplementedType,
        types: &RustTypes,
    ) -> Vec<Fragment> {
        let name = NameMangler::value(self.program.bindings.get(structural.method).name());
        let mut parameters = vec![format!("&{}", Self::RECEIVER)];
        parameters.extend(
            self.parameter_names(structural)
                .iter()
                .skip(1)
                .zip(&structural.parameters)
                .map(|(name, ty)| {
                    format!(
                        "{name}: {}",
                        types.show_in(ty, TypePosition::Signature, &LifetimeScope::default())
                    )
                }),
        );
        let result = types.show(&structural.result, TypePosition::Signature);
        let mut method = vec![
            Fragment::Newline,
            Fragment::Text(format!(
                "fn {name}({}) -> {result} {{",
                parameters.join(", ")
            )),
        ];
        method.push(Fragment::Indented(self.body(structural, implemented)));
        method.extend([Fragment::Newline, Fragment::Text("}".to_string())]);
        method
    }

    fn parameter_names(&self, structural: &StructuralTrait) -> Vec<String> {
        let mut names = vec![Self::RECEIVER.to_string()];
        names.extend(ParameterNames::new(&self.program.bindings).of(&structural.parameters));
        names
    }

    fn body(&self, structural: &StructuralTrait, implemented: &ImplementedType) -> Vec<Fragment> {
        match self.program.structs.get(implemented.binding) {
            Some(definition) => vec![
                Fragment::Newline,
                Fragment::Text(self.struct_body(structural, implemented, &definition.fields)),
            ],
            None => self.enum_body(structural, implemented),
        }
    }

    fn struct_body(
        &self,
        structural: &StructuralTrait,
        implemented: &ImplementedType,
        fields: &[(Rc<str>, Type)],
    ) -> String {
        let field_template = structural.clause.template(StructuralKey::Field);
        let parts: String = fields
            .iter()
            .map(|(field, _)| {
                field_template.fill(&TemplateValues {
                    field: Self::escaped(field),
                    value: format!("&{}.{}", Self::RECEIVER, NameMangler::value(field)),
                    ..TemplateValues::default()
                })
            })
            .collect();
        structural
            .clause
            .template(StructuralKey::Struct)
            .fill(&TemplateValues {
                parameters: self.parameter_names(structural),
                name: Self::escaped(self.program.bindings.get(implemented.binding).name()),
                parts,
                ..TemplateValues::default()
            })
    }

    fn enum_body(
        &self,
        structural: &StructuralTrait,
        implemented: &ImplementedType,
    ) -> Vec<Fragment> {
        let variants = self
            .program
            .enums
            .get(implemented.binding)
            .map_or(&[][..], |definition| &definition.variants);
        if variants.is_empty() {
            return vec![
                Fragment::Newline,
                Fragment::Text(format!("match *{} {{}}", Self::RECEIVER)),
            ];
        }
        let arms = variants
            .iter()
            .enumerate()
            .flat_map(|(position, variant)| {
                [
                    Fragment::Newline,
                    Fragment::Text(self.arm(structural, implemented, position, variant)),
                ]
            })
            .collect();
        vec![
            Fragment::Newline,
            Fragment::Text(format!("match {} {{", Self::RECEIVER)),
            Fragment::Indented(arms),
            Fragment::Newline,
            Fragment::Text("}".to_string()),
        ]
    }

    fn arm(
        &self,
        structural: &StructuralTrait,
        implemented: &ImplementedType,
        position: usize,
        variant: &VariantDefinition,
    ) -> String {
        let enum_name = self.program.bindings.get(implemented.binding).name();
        let qualified = format!("{enum_name}{}{}", Self::QUALIFIER, variant.name);
        let path = format!(
            "{}::{}",
            implemented.rust_name,
            NameMangler::variant_name(&variant.name)
        );
        let values = TemplateValues {
            parameters: self.parameter_names(structural),
            name: Self::escaped(&qualified),
            parts: self.variant_parts(structural, implemented.binding, position, variant),
            ..TemplateValues::default()
        };
        let (pattern, key) = match &variant.payload {
            VariantPayload::None => (path, StructuralKey::Bare),
            VariantPayload::Positional(payloads) => (
                format!("{path}({})", Self::part_names(payloads.len()).join(", ")),
                StructuralKey::Variant,
            ),
            VariantPayload::Named(fields) => (
                format!("{path} {{ {} }}", Self::named_parts(fields).join(", ")),
                StructuralKey::Variant,
            ),
        };
        format!(
            "{pattern} => {},",
            structural.clause.template(key).fill(&values)
        )
    }

    fn part_names(count: usize) -> Vec<String> {
        (0..count)
            .map(|position| format!("{}{position}", Self::PART_PREFIX))
            .collect()
    }

    fn named_parts(fields: &[(Rc<str>, Type)]) -> Vec<String> {
        fields
            .iter()
            .zip(Self::part_names(fields.len()))
            .map(|((field, _), part)| format!("{}: {part}", NameMangler::value(field)))
            .collect()
    }

    fn variant_parts(
        &self,
        structural: &StructuralTrait,
        binding: BindingId,
        position: usize,
        variant: &VariantDefinition,
    ) -> String {
        let part_template = structural.clause.template(StructuralKey::Part);
        Self::part_names(variant.payload_types().len())
            .into_iter()
            .enumerate()
            .map(|(payload, part)| {
                let boxed = self
                    .program
                    .inferred_boxes
                    .contains(&(binding, position, payload));
                let value = if boxed { format!("&**{part}") } else { part };
                part_template.fill(&TemplateValues {
                    value,
                    ..TemplateValues::default()
                })
            })
            .collect()
    }

    fn escaped(name: &str) -> String {
        name.escape_debug().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{BindingInfo, BindingKind, PrimitiveType};
    use crate::types::GenericId;

    struct NamingFixture {
        bindings: BindingArena,
        formatter: BindingId,
    }

    impl NamingFixture {
        fn new() -> Self {
            let mut bindings = BindingArena::default();
            let formatter = bindings.add(BindingInfo::new(
                "formatter".into(),
                BindingKind::TypeVariable,
                None,
            ));
            Self {
                bindings,
                formatter,
            }
        }

        fn names(&self, parameters: &[Type]) -> Vec<String> {
            ParameterNames::new(&self.bindings).of(parameters)
        }

        fn formatter(&self) -> Type {
            Type::mutable(Type::Named(self.formatter, Vec::new()))
        }
    }

    #[test]
    fn a_lone_parameter_takes_its_type_name() {
        let fixture = NamingFixture::new();
        assert_eq!(fixture.names(&[fixture.formatter()]), vec!["formatter"]);
    }

    #[test]
    fn several_parameters_take_their_positions() {
        let fixture = NamingFixture::new();
        let parameters = [
            fixture.formatter(),
            Type::Primitive(PrimitiveType::I64),
            Type::Generic(GenericId::new(0)),
        ];
        assert_eq!(
            fixture.names(&parameters),
            vec!["formatter_1", "i64_2", "parameter_3"]
        );
    }
}
