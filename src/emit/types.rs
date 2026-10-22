use std::collections::HashSet;

use super::mangle::NameMangler;
use super::plan::EmitPlan;
use crate::core::{BindingId, ModuleId, Piece, PrimitiveType};
use crate::types::{Bound, GenericId, GenericParam, RustType, Type, TypedProgram};

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
    const MAYBE_UNSIZED: &'static str = "?Sized";
    const INFERRED: &'static str = "_";
    const NUMBER_TRAIT: &'static str = "crate::rt::Number";
    const RUNTIME_SUPERTRAITS: [(&'static str, &'static [&'static str]); 4] = [
        ("crate::rt::Number", &["Copy", "PartialOrd", "PartialEq"]),
        (
            "crate::rt::Signed",
            &["crate::rt::Number", "Copy", "PartialOrd", "PartialEq"],
        ),
        (
            "crate::rt::Float",
            &[
                "crate::rt::Signed",
                "crate::rt::Number",
                "Copy",
                "PartialOrd",
                "PartialEq",
            ],
        ),
        (
            "crate::rt::Integer",
            &["crate::rt::Number", "Copy", "PartialOrd", "PartialEq"],
        ),
    ];

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
            Type::Named(binding, arguments) => self.named(*binding, arguments, position),
            Type::Function(parameters, result) => self.function(parameters, result, position),
            Type::Ref(inner) | Type::Mut(inner) => self.reference(shown, inner, position),
        }
    }

    fn reference(&self, reference: &Type, inner: &Type, position: TypePosition) -> String {
        let mutability = if matches!(reference, Type::Mut(_)) {
            "mut "
        } else {
            ""
        };
        format!(
            "&{}{mutability}{}",
            Self::lifetime(position),
            self.show(inner, Self::inner(position))
        )
    }

    pub fn generics(
        &self,
        params: &[GenericParam],
        signature: &Type,
        literal_generics: &HashSet<GenericId>,
    ) -> String {
        if params.is_empty() {
            return String::new();
        }
        let declared: Vec<String> = params
            .iter()
            .map(|param| {
                self.generic_declaration(param, signature, literal_generics.contains(&param.id))
            })
            .collect();
        format!("<{}>", declared.join(", "))
    }

    pub fn is_unit(shown: &Type) -> bool {
        *shown == Type::unit()
    }

    pub fn generic_name(param: &GenericParam) -> String {
        NameMangler::type_name(&param.name)
    }

    fn generic_declaration(
        &self,
        param: &GenericParam,
        signature: &Type,
        converts_literals: bool,
    ) -> String {
        let name = Self::generic_name(param);
        let declared = self.trait_paths(&param.bounds);
        let mut bounds = declared.clone();
        let implied = self
            .program
            .implied_bounds
            .get(&param.id)
            .map(|implied| self.trait_paths(implied))
            .unwrap_or_default();
        for path in implied {
            if !Self::is_covered(&path, &bounds) {
                bounds.push(path);
            }
        }
        if converts_literals && !Self::is_covered(Self::NUMBER_TRAIT, &bounds) {
            bounds.push(Self::NUMBER_TRAIT.to_string());
        }
        if Self::only_behind_references(signature, param.id, false) {
            bounds.push(Self::MAYBE_UNSIZED.to_string());
        }
        if bounds.is_empty() {
            name
        } else {
            format!("{name}: {}", bounds.join(" + "))
        }
    }

    fn trait_paths(&self, bounds: &[Bound]) -> Vec<String> {
        bounds
            .iter()
            .filter_map(|bound| {
                self.program
                    .traits
                    .get(bound.binding)
                    .map(|rust_trait| rust_trait.path.to_string())
            })
            .collect()
    }

    fn is_covered(path: &str, bounds: &[String]) -> bool {
        bounds
            .iter()
            .any(|bound| bound == path || Self::supertraits(bound).contains(&path))
    }

    fn supertraits(path: &str) -> &'static [&'static str] {
        Self::RUNTIME_SUPERTRAITS
            .iter()
            .find(|(runtime_trait, _)| *runtime_trait == path)
            .map_or(&[], |(_, supertraits)| *supertraits)
    }

    fn only_behind_references(
        signature: &Type,
        generic: GenericId,
        behind_reference: bool,
    ) -> bool {
        match signature {
            Type::Generic(found) if *found == generic => behind_reference,
            Type::Ref(inner) | Type::Mut(inner) => {
                Self::only_behind_references(inner, generic, true)
            }
            Type::Named(_, parts) => parts
                .iter()
                .all(|part| Self::only_behind_references(part, generic, false)),
            Type::Function(parameters, result) => parameters
                .iter()
                .chain(std::iter::once(result.as_ref()))
                .all(|part| Self::only_behind_references(part, generic, false)),
            Type::Var(_) | Type::Generic(_) | Type::Primitive(_) => true,
        }
    }

    fn named(&self, binding: BindingId, arguments: &[Type], position: TypePosition) -> String {
        let arguments: Vec<String> = arguments
            .iter()
            .map(|argument| self.show(argument, Self::inner(position)))
            .collect();
        match self.program.rust_types.get(binding) {
            Some(rust_type) => Self::rust_type(rust_type, &arguments, position),
            None => Self::applied(self.struct_path(binding), &arguments),
        }
    }

    fn rust_type(rust_type: &RustType, arguments: &[String], position: TypePosition) -> String {
        if rust_type.is_opaque() && position == TypePosition::Value {
            return Self::INFERRED.to_string();
        }
        rust_type
            .template
            .pieces()
            .iter()
            .map(|piece| match piece {
                Piece::Literal(text) => text.as_str(),
                Piece::Placeholder { index, .. } => arguments[*index].as_str(),
            })
            .collect()
    }

    fn applied(path: String, arguments: &[String]) -> String {
        if arguments.is_empty() {
            return path;
        }
        format!("{path}<{}>", arguments.join(", "))
    }

    pub fn is_opaque(&self, binding: BindingId) -> bool {
        self.program
            .rust_types
            .get(binding)
            .is_some_and(RustType::is_opaque)
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

    pub fn struct_path(&self, binding: BindingId) -> String {
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
