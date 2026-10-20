use super::mangle::NameMangler;
use super::plan::EmitPlan;
use crate::core::{BindingId, ModuleId, PrimitiveType};
use crate::types::{GenericParam, Type, TypedProgram};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TypePosition {
    Signature,
    StaticResult,
    Value,
}

pub struct RustTypes<'a> {
    program: &'a TypedProgram,
    plan: &'a EmitPlan<'a>,
    module: ModuleId,
}

impl<'a> RustTypes<'a> {
    pub fn new(program: &'a TypedProgram, plan: &'a EmitPlan<'a>, module: ModuleId) -> Self {
        Self {
            program,
            plan,
            module,
        }
    }

    pub fn show(&self, shown: &Type, position: TypePosition) -> String {
        match shown {
            Type::Var(_) => "_".to_string(),
            Type::Generic(generic) => self
                .program
                .generics
                .get(generic)
                .map_or_else(|| "_".to_string(), Self::generic_name),
            Type::Primitive(primitive) => Self::primitive(*primitive).to_string(),
            Type::Struct(binding) => self.struct_path(*binding),
            Type::Function(parameters, result) => self.function(parameters, result, position),
            Type::Ref(inner) => format!(
                "&{}{}",
                Self::lifetime(position),
                self.show(inner, Self::inner(position))
            ),
            Type::Mut(inner) => format!(
                "&{}mut {}",
                Self::lifetime(position),
                self.show(inner, Self::inner(position))
            ),
        }
    }

    pub fn generics(&self, params: &[GenericParam]) -> String {
        if params.is_empty() {
            return String::new();
        }
        let declared: Vec<String> = params
            .iter()
            .map(|param| self.generic_declaration(param))
            .collect();
        format!("<{}>", declared.join(", "))
    }

    pub fn is_unit(shown: &Type) -> bool {
        *shown == Type::unit()
    }

    pub fn generic_name(param: &GenericParam) -> String {
        NameMangler::type_name(&param.name)
    }

    fn generic_declaration(&self, param: &GenericParam) -> String {
        let name = Self::generic_name(param);
        let bounds: Vec<String> = param
            .bounds
            .iter()
            .filter_map(|bound| {
                self.program
                    .traits
                    .get(*bound)
                    .map(|rust_trait| rust_trait.path.to_string())
            })
            .collect();
        if bounds.is_empty() {
            name
        } else {
            format!("{name}: {}", bounds.join(" + "))
        }
    }

    fn function(&self, parameters: &[Type], result: &Type, position: TypePosition) -> String {
        let parameters: Vec<String> = parameters
            .iter()
            .map(|parameter| self.show(parameter, TypePosition::Value))
            .collect();
        let result = if Self::is_unit(result) {
            String::new()
        } else {
            format!(" -> {}", self.show(result, TypePosition::Value))
        };
        let keyword = match position {
            TypePosition::Signature | TypePosition::StaticResult => "impl Fn",
            TypePosition::Value => "fn",
        };
        format!("{keyword}({}){result}", parameters.join(", "))
    }

    fn lifetime(position: TypePosition) -> &'static str {
        match position {
            TypePosition::StaticResult => "'static ",
            TypePosition::Signature | TypePosition::Value => "",
        }
    }

    fn inner(position: TypePosition) -> TypePosition {
        match position {
            TypePosition::StaticResult => TypePosition::StaticResult,
            TypePosition::Signature | TypePosition::Value => TypePosition::Value,
        }
    }

    fn struct_path(&self, binding: BindingId) -> String {
        match self.plan.item(binding) {
            Some(_) => self.plan.path_to(binding, self.module),
            None => NameMangler::type_name(self.program.bindings.get(binding).name()),
        }
    }

    fn primitive(primitive: PrimitiveType) -> &'static str {
        match primitive {
            PrimitiveType::String => "String",
            PrimitiveType::Unit => "()",
            other => other.name(),
        }
    }
}
