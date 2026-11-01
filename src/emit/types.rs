use std::borrow::Cow;
use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::fresh_lifetimes::FreshLifetimes;
use super::function_borrow::FunctionBorrow;
use super::mangle::NameMangler;
use super::opaque_borrows::OpaqueBorrows;
use super::plan::EmitPlan;
use super::unsized_parameters::UnsizedParameters;
use crate::core::{BindingId, ModuleId, Piece, PrimitiveType};
use crate::types::{
    Bound, ClosureKind, GenericId, GenericParam, Lifetime, RustType, Type, TypedProgram,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TypePosition {
    Signature,
    StaticResult,
    Value,
}

#[derive(Default, Clone)]
pub struct LifetimeScope {
    names: FxHashMap<BindingId, String>,
    elided: Option<String>,
    elided_written: FxHashSet<BindingId>,
    opaque_lifetimes: Option<Rc<FreshLifetimes>>,
    names_function_lifetime: bool,
    reserved: Option<String>,
    elides_outer_reference: bool,
}

impl LifetimeScope {
    pub fn new(names: FxHashMap<BindingId, String>) -> Self {
        Self {
            names,
            elided: None,
            elided_written: FxHashSet::default(),
            opaque_lifetimes: None,
            names_function_lifetime: false,
            reserved: None,
            elides_outer_reference: false,
        }
    }

    pub fn naming_opaque_lifetimes(mut self, fresh: &Rc<FreshLifetimes>) -> Self {
        self.opaque_lifetimes = Some(Rc::clone(fresh));
        self
    }

    pub fn naming_signature_lifetimes(self, fresh: &Rc<FreshLifetimes>) -> Self {
        self.naming_opaque_lifetimes(fresh)
            .naming_function_lifetime()
    }

    pub fn naming_function_lifetime(mut self) -> Self {
        self.names_function_lifetime = true;
        self
    }

    fn entering_function(&self) -> Option<Self> {
        self.names_function_lifetime
            .then(|| self.entering_opaque())
            .flatten()
    }

    fn entering_opaque(&self) -> Option<Self> {
        let fresh = self
            .opaque_lifetimes
            .as_ref()
            .filter(|_| self.elided.is_none())?;
        Some(self.clone().with_elided(Some(fresh.issue())))
    }

    pub fn with_elided(mut self, elided: Option<String>) -> Self {
        self.elided = elided;
        self
    }

    pub fn reserving(mut self, declared: Option<&str>) -> Self {
        self.reserved = declared.map(str::to_string);
        self
    }

    pub fn eliding_outer_reference(mut self) -> Self {
        self.elides_outer_reference = true;
        self
    }

    fn entering_reference(&self) -> Cow<'_, Self> {
        if !self.elides_outer_reference {
            return Cow::Borrowed(self);
        }
        let mut inner = self.clone();
        inner.elides_outer_reference = false;
        Cow::Owned(inner)
    }

    pub fn eliding(mut self, binding: BindingId) -> Self {
        self.elided_written.insert(binding);
        self
    }

    pub fn name_of(&self, binding: BindingId) -> Option<String> {
        self.names.get(&binding).cloned()
    }

    pub fn is_elided(&self, binding: BindingId) -> bool {
        self.elided_written.contains(&binding)
    }

    pub fn elided(&self) -> Option<&str> {
        self.elided.as_deref()
    }

    pub fn taken_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .names
            .values()
            .chain(&self.elided)
            .chain(&self.reserved)
            .cloned()
            .collect();
        if let Some(fresh) = &self.opaque_lifetimes {
            names.extend(fresh.issued());
        }
        names
    }
}

pub struct RustTypes<'a> {
    program: &'a TypedProgram,
    plan: &'a EmitPlan<'a>,
    unsized_parameters: &'a UnsizedParameters,
    module: ModuleId,
}

impl<'a> RustTypes<'a> {
    pub const SLICE_TYPE: &'static str = RustType::SLICE_TEMPLATE;
    const MAYBE_UNSIZED: &'static str = "?Sized";
    const SIZED_TRAITS: [&'static str; 3] = ["Clone", "Copy", "Default"];
    const INFERRED: &'static str = "_";
    const ELIDED_LIFETIME_ARGUMENT: &'static str = "'_";
    const STATIC_LIFETIME: &'static str = "'static";
    const NUMBER_TRAIT: &'static str = "crate::rt::Number";
    const PREDICATE_SUBJECT: &'static str = "{0}";
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

    pub fn new(
        program: &'a TypedProgram,
        plan: &'a EmitPlan<'a>,
        unsized_parameters: &'a UnsizedParameters,
        module: ModuleId,
    ) -> Self {
        Self {
            program,
            plan,
            unsized_parameters,
            module,
        }
    }

    pub fn is_rust_type(program: &TypedProgram, binding: BindingId, expected: &str) -> bool {
        program
            .rust_types
            .get(binding)
            .is_some_and(|rust_type| rust_type.has_template(expected))
    }

    pub fn show(&self, shown: &Type, position: TypePosition) -> String {
        self.show_in(shown, position, &LifetimeScope::default())
    }

    fn generic(&self, generic: GenericId) -> String {
        if let Some(name) = self.program.associated_generics.get(&generic) {
            return format!("Self::{}", NameMangler::type_name(name));
        }
        self.program
            .generics
            .get(&generic)
            .map_or_else(|| "_".to_string(), Self::generic_name)
    }

    pub fn show_in(&self, shown: &Type, position: TypePosition, scope: &LifetimeScope) -> String {
        match shown {
            Type::Var(_) => "_".to_string(),
            Type::Generic(generic) => self.generic(*generic),
            Type::TraitObject(binding) => {
                format!("dyn {}", self.plan.trait_path(*binding, self.module))
            }
            _ => self.composite(shown, position, scope),
        }
    }

    fn composite(&self, shown: &Type, position: TypePosition, scope: &LifetimeScope) -> String {
        match shown {
            Type::Primitive(primitive) => Self::primitive(*primitive).to_string(),
            Type::Named(binding, arguments) => self.named(*binding, arguments, position, scope),
            Type::Function(kind, parameters, result) => {
                self.function(*kind, parameters, result, position, scope)
            }
            Type::Ref(lifetime, inner) | Type::Mut(lifetime, inner) => {
                self.reference(shown, *lifetime, inner, position, scope)
            }
            _ => unreachable!("the caller matches a composite type"),
        }
    }

    fn reference(
        &self,
        reference: &Type,
        lifetime: Lifetime,
        inner: &Type,
        position: TypePosition,
        scope: &LifetimeScope,
    ) -> String {
        let mutability = if matches!(reference, Type::Mut(_, _)) {
            "mut "
        } else {
            ""
        };
        format!(
            "&{}{mutability}{}",
            self.outer_lifetime(lifetime, position, scope),
            self.show_in(inner, Self::inner(position), &scope.entering_reference())
        )
    }

    fn outer_lifetime(
        &self,
        lifetime: Lifetime,
        position: TypePosition,
        scope: &LifetimeScope,
    ) -> String {
        if scope.elides_outer_reference && lifetime == Lifetime::Elided {
            return String::new();
        }
        self.lifetime(lifetime, position, scope)
    }

    pub fn generics(
        &self,
        params: &[GenericParam],
        signature: &Type,
        literal_generics: &FxHashSet<GenericId>,
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

    pub fn where_clause(&self, params: &[GenericParam]) -> String {
        let predicates: Vec<String> = params
            .iter()
            .flat_map(|param| {
                let name = Self::generic_name(param);
                self.bound_paths(param)
                    .into_iter()
                    .filter(|path| Self::is_predicate(path))
                    .map(move |predicate| predicate.replace(Self::PREDICATE_SUBJECT, &name))
            })
            .collect();
        if predicates.is_empty() {
            return String::new();
        }
        format!(" where {}", predicates.join(", "))
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
        let mut bounds = self.bound_paths(param);
        bounds.retain(|path| !Self::is_predicate(path));
        if converts_literals && !Self::is_covered(Self::NUMBER_TRAIT, &bounds) {
            bounds.push(Self::NUMBER_TRAIT.to_string());
        }
        if self.needs_maybe_unsized(param, signature, converts_literals) {
            bounds.push(Self::MAYBE_UNSIZED.to_string());
        }
        if bounds.is_empty() {
            name
        } else {
            format!("{name}: {}", bounds.join(" + "))
        }
    }

    fn bound_paths(&self, param: &GenericParam) -> Vec<String> {
        let mut bounds = self.declared_bounds(param);
        self.add_implied_bounds(param, &mut bounds);
        bounds
    }

    fn is_predicate(path: &str) -> bool {
        path.contains(Self::PREDICATE_SUBJECT)
    }

    fn needs_maybe_unsized(
        &self,
        param: &GenericParam,
        signature: &Type,
        converts_literals: bool,
    ) -> bool {
        Self::only_behind_references(signature, param.id, false)
            && self.unsized_parameters.contains(param.id)
            && !converts_literals
            && !self.requires_sized(param)
    }

    fn requires_sized(&self, param: &GenericParam) -> bool {
        param
            .bounds
            .iter()
            .chain(
                self.program
                    .implied_bounds
                    .get(&param.id)
                    .into_iter()
                    .flatten(),
            )
            .any(|bound| self.bound_requires_sized(bound))
    }

    fn bound_requires_sized(&self, bound: &Bound) -> bool {
        self.program
            .traits
            .get(bound.binding)
            .is_some_and(|rust_trait| {
                Self::SIZED_TRAITS.contains(&rust_trait.path.as_ref())
                    || Self::supertraits(&rust_trait.path).contains(&"Copy")
            })
    }

    fn declared_bounds(&self, param: &GenericParam) -> Vec<String> {
        param
            .bounds
            .iter()
            .filter(|bound| {
                !param
                    .bounds
                    .iter()
                    .any(|other| other.binding != bound.binding && self.bound_implies(other, bound))
            })
            .filter_map(|bound| self.trait_path(bound))
            .collect()
    }

    fn add_implied_bounds(&self, param: &GenericParam, bounds: &mut Vec<String>) {
        let implied = self
            .program
            .implied_bounds
            .get(&param.id)
            .cloned()
            .unwrap_or_default();
        for implied in &implied {
            if param
                .bounds
                .iter()
                .any(|bound| self.bound_implies(bound, implied))
            {
                continue;
            }
            if let Some(path) = self.trait_path(implied)
                && !Self::is_covered(&path, bounds)
            {
                bounds.push(path);
            }
        }
    }

    fn trait_path(&self, bound: &Bound) -> Option<String> {
        let rust_trait = self.program.traits.get(bound.binding)?;
        let path = rust_trait.path.to_string();
        Some(match (&bound.argument, &rust_trait.associated) {
            (Some(argument), Some(associated)) => format!(
                "{path}<{} = {}>",
                NameMangler::type_name(associated),
                self.show(argument, TypePosition::Value)
            ),
            (Some(argument), None) => {
                format!("{path}<{}>", self.show(argument, TypePosition::Value))
            }
            (None, _) => path,
        })
    }

    fn bound_implies(&self, declared: &Bound, required: &Bound) -> bool {
        if declared == required {
            return true;
        }
        self.program
            .traits
            .get(declared.binding)
            .is_some_and(|rust_trait| {
                rust_trait
                    .supertraits
                    .iter()
                    .any(|supertrait| self.bound_implies(supertrait, required))
            })
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
            Type::Ref(_, inner) | Type::Mut(_, inner) => {
                Self::only_behind_references(inner, generic, true)
            }
            Type::Named(_, parts) => parts
                .iter()
                .all(|part| Self::only_behind_references(part, generic, false)),
            Type::Function(_, parameters, result) => parameters
                .iter()
                .chain(std::iter::once(result.as_ref()))
                .all(|part| Self::only_behind_references(part, generic, false)),
            Type::Var(_) | Type::Generic(_) | Type::Primitive(_) | Type::TraitObject(_) => true,
        }
    }

    fn named(
        &self,
        binding: BindingId,
        arguments: &[Type],
        position: TypePosition,
        scope: &LifetimeScope,
    ) -> String {
        if self.program.traits.projections().is_projection(binding) {
            return self.projection(binding, arguments, position, scope);
        }
        match self.program.rust_types.get(binding) {
            Some(rust_type) => self.rust_type(rust_type, arguments, position, scope),
            None => {
                let mut arguments: Vec<String> = arguments
                    .iter()
                    .map(|argument| self.show_in(argument, Self::inner(position), scope))
                    .collect();
                let lifetime_arguments = self.lifetime_parameter_count(binding);
                for _ in 0..lifetime_arguments {
                    arguments.insert(0, Self::lifetime_argument(position, scope));
                }
                Self::applied(self.struct_path(binding), &arguments)
            }
        }
    }

    fn projection(
        &self,
        binding: BindingId,
        arguments: &[Type],
        position: TypePosition,
        scope: &LifetimeScope,
    ) -> String {
        let projections = self.program.traits.projections();
        let projected = Type::Named(binding, arguments.to_vec());
        if let Some(normalized) = projections.normalize(&projected) {
            return self.show_in(&normalized, position, scope);
        }
        let associated = projections
            .associated(binding)
            .expect("a projection names a declared associated type");
        let owner = &arguments[0];
        let trait_path = self
            .owner_bound(owner, associated.trait_binding)
            .and_then(|bound| self.trait_path(bound))
            .unwrap_or_else(|| self.plan.trait_path(associated.trait_binding, self.module));
        format!(
            "<{} as {trait_path}>::{}",
            self.show_in(owner, Self::inner(position), scope),
            associated.rust_name
        )
    }

    fn owner_bound(&self, owner: &Type, trait_binding: BindingId) -> Option<&Bound> {
        let Type::Generic(generic) = owner else {
            return None;
        };
        self.program
            .generics
            .get(generic)?
            .bounds
            .iter()
            .find(|bound| bound.binding == trait_binding)
    }

    fn rust_type(
        &self,
        rust_type: &RustType,
        arguments: &[Type],
        position: TypePosition,
        scope: &LifetimeScope,
    ) -> String {
        if rust_type.is_opaque() && position == TypePosition::Value {
            return Self::INFERRED.to_string();
        }
        let template = rust_type.template_for(arguments.first());
        let argument_scope = self.argument_scope(rust_type, arguments, position, scope);
        template
            .pieces()
            .iter()
            .map(|piece| match piece {
                Piece::Literal(text) => text.to_string(),
                Piece::Placeholder { index, .. } => {
                    self.show_in(&arguments[*index], Self::inner(position), &argument_scope)
                }
                Piece::TypePlaceholder { .. } | Piece::BinderPlaceholder { .. } => "_".to_string(),
            })
            .collect()
    }

    fn argument_scope<'scope>(
        &self,
        rust_type: &RustType,
        arguments: &[Type],
        position: TypePosition,
        scope: &'scope LifetimeScope,
    ) -> Cow<'scope, LifetimeScope> {
        self.hides_elided_borrow(rust_type, arguments, position)
            .then(|| scope.entering_opaque())
            .flatten()
            .map_or(Cow::Borrowed(scope), Cow::Owned)
    }

    fn hides_elided_borrow(
        &self,
        rust_type: &RustType,
        arguments: &[Type],
        position: TypePosition,
    ) -> bool {
        let borrows = OpaqueBorrows::new(self.program);
        rust_type.is_opaque()
            && position == TypePosition::Signature
            && arguments
                .iter()
                .any(|argument| borrows.holds_elided_borrow(argument))
    }

    fn lifetime_parameter_count(&self, binding: BindingId) -> usize {
        let lifetimes = self
            .program
            .structs
            .get(binding)
            .map(|definition| &definition.lifetimes)
            .or_else(|| {
                self.program
                    .enums
                    .get(binding)
                    .map(|definition| &definition.lifetimes)
            });
        lifetimes.map_or(0, |lifetimes| {
            if lifetimes.is_empty() {
                usize::from(self.program.definition_holds_borrow(binding))
            } else {
                lifetimes.len()
            }
        })
    }

    fn lifetime_argument(position: TypePosition, scope: &LifetimeScope) -> String {
        match position {
            TypePosition::StaticResult => Self::STATIC_LIFETIME.to_string(),
            TypePosition::Signature | TypePosition::Value => scope.elided().map_or_else(
                || Self::ELIDED_LIFETIME_ARGUMENT.to_string(),
                str::to_string,
            ),
        }
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

    fn function(
        &self,
        kind: ClosureKind,
        parameters: &[Type],
        result: &Type,
        position: TypePosition,
        scope: &LifetimeScope,
    ) -> String {
        let scope = self.function_parameter_scope(result, position, scope);
        let (inner_scope, binder) = self.function_binder(parameters, result, position, &scope);
        let keyword = match position {
            TypePosition::Signature | TypePosition::StaticResult => {
                format!("impl {binder}{}", kind.keyword())
            }
            TypePosition::Value => format!("{binder}fn"),
        };
        format!(
            "{keyword}{}",
            self.function_shape(parameters, result, &inner_scope)
        )
    }

    pub fn borrowed_function(
        &self,
        borrow: FunctionBorrow,
        parameters: &[Type],
        result: &Type,
        enclosing_lifetime: Option<&str>,
    ) -> String {
        let (position, scope) = Self::closure_parameter_scope(enclosing_lifetime);
        let (inner_scope, binder) = self.function_binder(parameters, result, position, &scope);
        format!(
            "{}{}",
            borrow.trait_object(&binder),
            self.function_shape(parameters, result, &inner_scope)
        )
    }

    pub(super) fn closure_parameter_scope(
        enclosing_lifetime: Option<&str>,
    ) -> (TypePosition, LifetimeScope) {
        match enclosing_lifetime {
            Some(lifetime) => (
                TypePosition::Value,
                LifetimeScope::default().with_elided(Some(lifetime.to_string())),
            ),
            None => (TypePosition::StaticResult, LifetimeScope::default()),
        }
    }

    fn function_binder(
        &self,
        parameters: &[Type],
        result: &Type,
        position: TypePosition,
        scope: &LifetimeScope,
    ) -> (LifetimeScope, String) {
        let (inner_scope, binder) = Self::function_scope(
            self.inherited_lifetime(parameters, result, position, scope),
            scope,
            self.program
                .function_type_needs_shared_lifetime(parameters, result),
        );
        let binder = binder.map_or_else(String::new, |lifetime| format!("for<{lifetime}> "));
        (inner_scope, binder)
    }

    fn function_shape(&self, parameters: &[Type], result: &Type, scope: &LifetimeScope) -> String {
        let parameters: Vec<String> = parameters
            .iter()
            .map(|parameter| self.show_in(parameter, TypePosition::Value, scope))
            .collect();
        let result = if Self::is_unit(result) {
            String::new()
        } else {
            format!(" -> {}", self.show_in(result, TypePosition::Value, scope))
        };
        format!("({}){result}", parameters.join(", "))
    }

    fn function_parameter_scope<'scope>(
        &self,
        result: &Type,
        position: TypePosition,
        scope: &'scope LifetimeScope,
    ) -> Cow<'scope, LifetimeScope> {
        let names_lifetime = position == TypePosition::Signature
            && self.program.function_type_returns_borrow(result);
        names_lifetime
            .then(|| scope.entering_function())
            .flatten()
            .map_or(Cow::Borrowed(scope), Cow::Owned)
    }

    fn inherited_lifetime(
        &self,
        parameters: &[Type],
        result: &Type,
        position: TypePosition,
        scope: &LifetimeScope,
    ) -> Option<String> {
        let needs_outer = self
            .program
            .function_type_needs_outer_lifetime(parameters, result);
        match position {
            TypePosition::Signature => scope.elided().map(str::to_string),
            TypePosition::StaticResult => needs_outer.then(|| Self::STATIC_LIFETIME.to_string()),
            TypePosition::Value => scope.elided().filter(|_| needs_outer).map(str::to_string),
        }
    }

    fn function_scope(
        inherited: Option<String>,
        scope: &LifetimeScope,
        needs_binder: bool,
    ) -> (LifetimeScope, Option<String>) {
        let binder = if inherited.is_none() && needs_binder {
            Some(Self::fresh_binder(scope))
        } else {
            None
        };
        let shared = inherited.or_else(|| binder.clone());
        (LifetimeScope::default().with_elided(shared), binder)
    }

    fn fresh_binder(scope: &LifetimeScope) -> String {
        FreshLifetimes::unused_name(&scope.taken_names())
    }

    fn lifetime(
        &self,
        lifetime: Lifetime,
        position: TypePosition,
        scope: &LifetimeScope,
    ) -> String {
        self.lifetime_name(lifetime, position, scope)
            .map_or_else(String::new, |name| format!("{name} "))
    }

    pub fn lifetime_name(
        &self,
        lifetime: Lifetime,
        position: TypePosition,
        scope: &LifetimeScope,
    ) -> Option<String> {
        match lifetime {
            Lifetime::Elided if position == TypePosition::StaticResult => {
                Some(Self::STATIC_LIFETIME.to_string())
            }
            Lifetime::Elided => scope.elided().map(str::to_string),
            Lifetime::Written(binding) if scope.is_elided(binding) => None,
            Lifetime::Written(binding) => Some(
                scope
                    .name_of(binding)
                    .unwrap_or_else(|| Self::binding_lifetime(self.program, binding)),
            ),
        }
    }

    pub fn reference_lifetime_name(
        &self,
        reference: &Type,
        position: TypePosition,
        scope: &LifetimeScope,
    ) -> Option<String> {
        self.lifetime_name(reference.reference_lifetime()?, position, scope)
    }

    pub fn binding_lifetime(program: &TypedProgram, binding: BindingId) -> String {
        NameMangler::lifetime(program.bindings.get(binding).name())
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
