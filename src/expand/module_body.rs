use std::collections::VecDeque;
use std::rc::Rc;

use super::Expander;
use super::TraitMethods;
use super::context::DefinitionContext;
use super::problems::ExpandProblems;
use super::shape::Shape;
use crate::core::{
    BindingId, BindingInfo, BindingKind, ControlForm, CoreModule, DefinitionForm, Expr, ExprKind,
    ExpressionForm, FieldLabel, ImplAssociatedType, ImplMethod, Item, ItemKind, KernelForm, Leaf,
    ModuleId, Namespace, RUST_TRAIT_ASSOCIATED_MARKER, RUST_TRAIT_HELP_MARKER,
    RUST_TRAIT_METHOD_MARKER, RustTemplate, RustTraitAssociatedType, RustTraitDeclaration,
    RustTraitImplementor, TraitAssociatedType, TraitMethod, TypeBinder, TypeBound, TypeExpr,
    TypeExprKind, VariantDeclaration, VariantInfo, VariantPayload,
};
use crate::diagnostic::{Counted, Diagnostic};
use crate::emit::mangle::NameMangler;
use crate::reader::{Form, FormKind, Symbol};
use crate::source::Span;

struct TraitLowering<'a, 'f> {
    shape: &'a Shape<'f>,
    binding: BindingId,
    self_variable: BindingId,
    rust_name: &'a Rc<str>,
    scope: crate::scope::Scope,
    associated: &'a [(BindingId, Rc<str>)],
}

struct ImplLowering<'a, 'f> {
    shape: &'a Shape<'f>,
    trait_methods: &'a TraitMethods,
    trait_name: &'a str,
    implementor: &'a TypeExpr,
    associated: &'a [(BindingId, Rc<str>, TypeExpr)],
}

type ImplAssociated = Vec<(BindingId, Rc<str>, TypeExpr)>;
type ImplLoweringParts = (ImplAssociated, Vec<Form>);

struct RustTraitEntries {
    methods: Vec<TraitMethod>,
    help: Option<Rc<str>>,
    associated: Vec<RustTraitAssociatedType>,
    implementors: Vec<RustTraitImplementor>,
}

enum PendingItem {
    Def {
        binding: BindingId,
        annotation: Option<Form>,
        span: Span,
    },
    Signature(Form),
    Derive(Form),
    Struct(BindingId, Form),
    Enum(BindingId, Form),
    RustTrait(BindingId, Form),
    RustType(BindingId, Form),
    Trait(BindingId, Form),
    Impl(crate::scope::Scope, Form),
    Export(Form),
}

impl Expander<'_> {
    pub(super) fn expand_module_body(&mut self, module: ModuleId, forms: Vec<Form>) {
        let outer_module = self.current_module.replace(module);
        let pending = self.register_definitions(module, forms);
        let mut core = CoreModule {
            id: module,
            path: self.modules.get(module).path().clone(),
            items: Vec::new(),
            exports: Vec::new(),
        };
        let (impls, rest): (Vec<PendingItem>, Vec<PendingItem>) = pending
            .into_iter()
            .partition(|item| matches!(item, PendingItem::Impl(..)));
        let (exports, rest): (Vec<PendingItem>, Vec<PendingItem>) = rest
            .into_iter()
            .partition(|item| matches!(item, PendingItem::Export(..)));
        self.lower_pending_group(module, rest, &mut core);
        self.lower_pending_group(module, impls, &mut core);
        self.lower_pending_group(module, exports, &mut core);
        self.compile_pending_macros();
        self.record_answering_macros(module);
        self.order.push(module);
        self.expanded.insert(module, core);
        self.current_module = outer_module;
    }

    fn lower_pending_group(
        &mut self,
        module: ModuleId,
        items: Vec<PendingItem>,
        core: &mut CoreModule,
    ) {
        for item in items {
            let lowered =
                self.within_top_level_form(|expander| expander.lower_pending(module, item, core));
            if let Err(diagnostic) = lowered {
                self.diagnostics.push(diagnostic);
            }
        }
    }

    pub(super) fn split_binder(binder: &Form) -> (&Form, Option<&Form>) {
        match binder.as_list() {
            Some([name, annotation]) if name.as_symbol().is_some() => (name, Some(annotation)),
            _ => (binder, None),
        }
    }

    fn register_definitions(&mut self, module: ModuleId, forms: Vec<Form>) -> Vec<PendingItem> {
        let mut context = DefinitionContext::default();
        let mut queue: VecDeque<Form> = forms.into();
        let mut pending = Vec::new();
        while let Some(form) = queue.pop_front() {
            let registered = self.within_top_level_form(|expander| {
                expander.register_top_level(module, form, &mut context, &mut queue)
            });
            match registered {
                Ok(item) => pending.extend(item),
                Err(diagnostic) => self.diagnostics.push(diagnostic),
            }
        }
        pending
    }

    fn register_top_level(
        &mut self,
        module: ModuleId,
        form: Form,
        context: &mut DefinitionContext,
        queue: &mut VecDeque<Form>,
    ) -> Result<Option<PendingItem>, Diagnostic> {
        let form = self.expand_head(form, Some(context))?;
        match self.definition_head(&form)? {
            KernelForm::Definition(definition) => {
                self.register_definition(definition, module, form, context)
            }
            KernelForm::Expression(ExpressionForm::Control(ControlForm::Do)) => {
                Self::splice(&form, queue);
                Ok(None)
            }
            _ => Err(ExpandProblems::not_a_definition(form.span())),
        }
    }

    fn register_definition(
        &mut self,
        definition: DefinitionForm,
        module: ModuleId,
        form: Form,
        context: &mut DefinitionContext,
    ) -> Result<Option<PendingItem>, Diagnostic> {
        match definition {
            DefinitionForm::Def => self.register_def(module, &form, context),
            DefinitionForm::Type => Ok(Some(PendingItem::Signature(form))),
            DefinitionForm::Struct => self.register_struct(module, form, context),
            DefinitionForm::RustTrait => self.register_rust_trait(module, form, context),
            DefinitionForm::RustType => self.register_rust_type(module, form, context),
            other => self.register_other_definition(module, other, form, context),
        }
    }

    fn register_other_definition(
        &mut self,
        module: ModuleId,
        definition: DefinitionForm,
        form: Form,
        context: &mut DefinitionContext,
    ) -> Result<Option<PendingItem>, Diagnostic> {
        match definition {
            DefinitionForm::Enum => self.register_enum(module, form, context),
            DefinitionForm::Derive => Ok(Some(PendingItem::Derive(form))),
            DefinitionForm::Trait => self.register_trait(module, form, context),
            DefinitionForm::Impl => self.register_impl(form),
            DefinitionForm::Use | DefinitionForm::Export => {
                self.register_module_definition(definition, form, context)
            }
            _ => unreachable!("the caller matches another definition form"),
        }
    }

    fn register_module_definition(
        &mut self,
        definition: DefinitionForm,
        form: Form,
        context: &mut DefinitionContext,
    ) -> Result<Option<PendingItem>, Diagnostic> {
        match definition {
            DefinitionForm::Use => self.import(&form, context).map(|()| None),
            DefinitionForm::Export => Ok(Some(PendingItem::Export(form))),
            _ => unreachable!("only `use` and `export` reach this helper"),
        }
    }

    fn register_struct(
        &mut self,
        module: ModuleId,
        form: Form,
        context: &mut DefinitionContext,
    ) -> Result<Option<PendingItem>, Diagnostic> {
        let binding = self.register_type_name(
            &form,
            KernelForm::Definition(DefinitionForm::Struct),
            BindingKind::Struct(module),
            context,
        )?;
        Ok(Some(PendingItem::Struct(binding, form)))
    }

    fn register_enum(
        &mut self,
        module: ModuleId,
        form: Form,
        context: &mut DefinitionContext,
    ) -> Result<Option<PendingItem>, Diagnostic> {
        let shape = Shape::new(KernelForm::Definition(DefinitionForm::Enum), &form);
        let items = shape.at_least(2)?;
        let name = self.head_name(&shape, &items[1])?;
        let binding =
            self.define_global(name, Namespace::Type, BindingKind::Enum(module), context)?;
        let first_variant = Self::first_enum_variant(items);
        let mut constructors = Vec::new();
        for (index, variant) in items[first_variant..].iter().enumerate() {
            let (variant_name, arity) = self.variant_shape(&shape, variant)?;
            constructors.push(self.define_variant(
                binding,
                name,
                variant_name,
                index,
                arity,
                context,
            )?);
        }
        self.enum_variants.insert(binding, constructors);
        Ok(Some(PendingItem::Enum(binding, form)))
    }

    fn first_enum_variant(items: &[Form]) -> usize {
        if items.get(2).is_some_and(Self::starts_rust_clause) {
            3
        } else {
            2
        }
    }

    fn define_variant(
        &mut self,
        owner: BindingId,
        enum_name: &Form,
        variant_name: &Form,
        index: usize,
        arity: usize,
        context: &DefinitionContext,
    ) -> Result<BindingId, Diagnostic> {
        let BindingKind::Enum(module) = self.kind(owner) else {
            unreachable!("an enum's owner is an enum binding");
        };
        let enum_symbol = enum_name
            .as_symbol()
            .ok_or_else(|| ExpandProblems::not_a_name(enum_name.span()))?;
        let variant_symbol = variant_name
            .as_symbol()
            .ok_or_else(|| ExpandProblems::not_a_name(variant_name.span()))?;
        let constructor = Symbol::with_scopes(
            format!("{}/{}", enum_symbol.name(), variant_symbol.name()),
            enum_symbol.scopes().clone(),
        );
        let binder = Form::new(FormKind::Symbol(constructor), variant_name.span());
        self.define_global(
            &binder,
            Namespace::Value,
            BindingKind::Variant(
                module,
                VariantInfo {
                    owner,
                    index,
                    arity,
                },
            ),
            context,
        )
    }

    fn variant_shape<'f>(
        &self,
        shape: &Shape,
        variant: &'f Form,
    ) -> Result<(&'f Form, usize), Diagnostic> {
        match variant.kind() {
            FormKind::Symbol(_) => Ok((variant, 0)),
            FormKind::List(items) => {
                let Some(name) = items.first() else {
                    return Err(shape.malformed(variant));
                };
                let payload = &items[1..];
                if let [single] = payload
                    && self.is_struct_payload(single)
                {
                    let fields = single.as_list().map_or(&[][..], |items| &items[1..]);
                    return Ok((name, fields.len()));
                }
                Ok((name, payload.len()))
            }
            FormKind::Number(_) | FormKind::String(_) => Err(shape.malformed(variant)),
        }
    }

    fn is_struct_payload(&self, form: &Form) -> bool {
        self.is_kernel_form(form, KernelForm::Definition(DefinitionForm::Struct))
    }

    fn register_rust_trait(
        &mut self,
        module: ModuleId,
        form: Form,
        context: &mut DefinitionContext,
    ) -> Result<Option<PendingItem>, Diagnostic> {
        let shape = Shape::new(KernelForm::Definition(DefinitionForm::RustTrait), &form);
        let binding = self.register_type_name(
            &form,
            KernelForm::Definition(DefinitionForm::RustTrait),
            BindingKind::Trait(module),
            context,
        )?;
        for entry in shape.items().iter().skip(3) {
            let Some([_, name, ..]) = entry.as_list() else {
                continue;
            };
            if Self::is_rust_trait_method(entry) {
                self.define_global(name, Namespace::Value, BindingKind::Global(module), context)?;
            } else if Self::is_rust_trait_associated(entry) {
                let associated = name.as_list().and_then(<[Form]>::first).unwrap_or(name);
                self.define_global(
                    associated,
                    Namespace::Type,
                    BindingKind::RustType(module),
                    context,
                )?;
            }
        }
        Ok(Some(PendingItem::RustTrait(binding, form)))
    }

    fn rust_trait_marker(entry: &Form) -> Option<&str> {
        entry
            .as_list()
            .and_then(|items| items.first())
            .and_then(|head| head.as_symbol())
            .map(Symbol::name)
    }

    fn is_rust_trait_method(entry: &Form) -> bool {
        Self::rust_trait_marker(entry) == Some(RUST_TRAIT_METHOD_MARKER)
    }

    fn is_rust_trait_help(entry: &Form) -> bool {
        Self::rust_trait_marker(entry) == Some(RUST_TRAIT_HELP_MARKER)
    }

    fn is_rust_trait_associated(entry: &Form) -> bool {
        Self::rust_trait_marker(entry) == Some(RUST_TRAIT_ASSOCIATED_MARKER)
    }

    fn register_rust_type(
        &mut self,
        module: ModuleId,
        form: Form,
        context: &mut DefinitionContext,
    ) -> Result<Option<PendingItem>, Diagnostic> {
        let binding = self.register_type_name(
            &form,
            KernelForm::Definition(DefinitionForm::RustType),
            BindingKind::RustType(module),
            context,
        )?;
        Ok(Some(PendingItem::RustType(binding, form)))
    }

    fn register_trait(
        &mut self,
        module: ModuleId,
        form: Form,
        context: &mut DefinitionContext,
    ) -> Result<Option<PendingItem>, Diagnostic> {
        let shape = Shape::new(KernelForm::Definition(DefinitionForm::Trait), &form);
        let items = shape.at_least(2)?;
        let name = self.head_name(&shape, &items[1])?;
        let binding =
            self.define_global(name, Namespace::Type, BindingKind::Trait(module), context)?;
        for method in &items[2..] {
            if Self::is_associated_entry(method) {
                continue;
            }
            let method_items = method.as_list().ok_or_else(|| shape.malformed(method))?;
            let [method_name, _, ..] = method_items else {
                return Err(shape.malformed(method));
            };
            let method = self.define_global(
                method_name,
                Namespace::Value,
                BindingKind::Global(module),
                context,
            );
            method?;
        }
        Ok(Some(PendingItem::Trait(binding, form)))
    }

    fn register_impl(&mut self, form: Form) -> Result<Option<PendingItem>, Diagnostic> {
        let mut form = form;
        let scope = self.fresh_local_scope();
        form.add_scope(scope);
        Ok(Some(PendingItem::Impl(scope, form)))
    }

    fn definition_head(&self, form: &Form) -> Result<KernelForm, Diagnostic> {
        let not_a_definition = || ExpandProblems::not_a_definition(form.span());
        let head = form
            .as_list()
            .and_then(<[Form]>::first)
            .ok_or_else(not_a_definition)?;
        let symbol = head.as_symbol().ok_or_else(not_a_definition)?;
        match self.kind(self.resolve(symbol, Namespace::Value, head.span())?) {
            BindingKind::KernelForm(kernel) => Ok(kernel),
            _ => Err(not_a_definition()),
        }
    }

    fn splice(form: &Form, queue: &mut VecDeque<Form>) {
        let items = form.as_list().unwrap_or_default();
        for item in items.iter().skip(1).rev() {
            queue.push_front(item.clone());
        }
    }

    fn register_def(
        &mut self,
        module: ModuleId,
        form: &Form,
        context: &DefinitionContext,
    ) -> Result<Option<PendingItem>, Diagnostic> {
        let items = Shape::new(KernelForm::Definition(DefinitionForm::Def), form).exactly(3)?;
        let (name, annotation) = Self::split_binder(&items[1]);
        let value = &items[2];
        if self.is_kernel_form(value, KernelForm::Macro) {
            let binding =
                self.define_global(name, Namespace::Value, BindingKind::Macro(module), context)?;
            self.macros.add_pending(binding, value.clone());
            return Ok(None);
        }
        let binding =
            self.define_global(name, Namespace::Value, BindingKind::Global(module), context)?;
        self.globals.add_pending(binding, value.clone());
        Ok(Some(PendingItem::Def {
            binding,
            annotation: annotation.cloned(),
            span: form.span(),
        }))
    }

    fn register_type_name(
        &mut self,
        form: &Form,
        kernel: KernelForm,
        kind: BindingKind,
        context: &DefinitionContext,
    ) -> Result<BindingId, Diagnostic> {
        let shape = Shape::new(kernel, form);
        let items = shape.at_least(2)?;
        let name = self.head_name(&shape, &items[1])?;
        self.define_global(name, Namespace::Type, kind, context)
    }

    fn lower_pending(
        &mut self,
        module: ModuleId,
        item: PendingItem,
        core: &mut CoreModule,
    ) -> Result<(), Diagnostic> {
        match item {
            PendingItem::Def {
                binding,
                annotation,
                span,
            } => self.lower_def(binding, annotation, span, core),
            PendingItem::Signature(form) => self.lower_signature(module, &form, core),
            PendingItem::Struct(binding, form) => self.lower_struct(binding, &form, core),
            PendingItem::RustTrait(binding, form) => self.lower_rust_trait(binding, &form, core),
            PendingItem::RustType(binding, form) => self.lower_rust_type(binding, &form, core),
            other => self.lower_other_pending(module, other, core),
        }
    }

    fn lower_other_pending(
        &mut self,
        module: ModuleId,
        item: PendingItem,
        core: &mut CoreModule,
    ) -> Result<(), Diagnostic> {
        match item {
            PendingItem::Enum(binding, form) => self.lower_enum(binding, &form, core),
            PendingItem::Derive(form) => self.lower_derive(module, &form, core),
            PendingItem::Trait(binding, form) => self.lower_trait(binding, &form, core),
            PendingItem::Impl(scope, form) => self.lower_impl(scope, &form, core),
            PendingItem::Export(form) => {
                self.lower_exports(module, &form, core);
                Ok(())
            }
            _ => unreachable!("the caller matches another pending item"),
        }
    }

    fn lower_def(
        &mut self,
        binding: BindingId,
        annotation: Option<Form>,
        span: Span,
        core: &mut CoreModule,
    ) -> Result<(), Diagnostic> {
        if self.globals.has_failed(binding) {
            return Ok(());
        }
        if let Some(annotation) = annotation {
            let signature = self.lower_type(&annotation)?;
            core.items.push(Item {
                kind: ItemKind::Signature(binding, signature),
                span,
                hidden: false,
            });
        }
        let value = self.lowered_global(binding, span)?;
        core.items.push(Item {
            kind: ItemKind::Def(binding, (*value).clone()),
            span,
            hidden: false,
        });
        Ok(())
    }

    fn lower_signature(
        &mut self,
        module: ModuleId,
        form: &Form,
        core: &mut CoreModule,
    ) -> Result<(), Diagnostic> {
        let shape = Shape::new(KernelForm::Definition(DefinitionForm::Type), form);
        let items = shape.exactly(3)?;
        let binding = self.signature_binding(module, &shape, &items[1])?;
        let signature = self.lower_type(&items[2])?;
        core.items.push(Item {
            kind: ItemKind::Signature(binding, signature),
            span: form.span(),
            hidden: false,
        });
        Ok(())
    }

    fn signature_binding(
        &mut self,
        module: ModuleId,
        shape: &Shape,
        name: &Form,
    ) -> Result<BindingId, Diagnostic> {
        let written = self.resolve(shape.symbol(name)?, Namespace::Value, name.span())?;
        let binding = self
            .answered_global(module, written, name)
            .unwrap_or(written);
        if self.kind(binding) != BindingKind::Global(module) {
            return Err(ExpandProblems::signature_target(
                self.bindings.get(binding).name(),
                name.span(),
            ));
        }
        Ok(binding)
    }

    fn lower_struct(
        &mut self,
        binding: BindingId,
        form: &Form,
        core: &mut CoreModule,
    ) -> Result<(), Diagnostic> {
        let shape = Shape::new(KernelForm::Definition(DefinitionForm::Struct), form);
        let items = shape.at_least(2)?;
        let body = items[2..].to_vec();
        if let Some(where_items) = self.old_parameters(&body) {
            let span = where_items[0].span();
            let kind = self
                .macro_origin_at(span)
                .map_or("struct", |origin| origin.name.as_ref());
            let replacement = self.old_head_replacement(kind, &items[1], where_items);
            return Err(ExpandProblems::parameters_on_name(kind, &replacement, span));
        }
        let parameterized = self.lower_head(&shape, &items[1], body)?;
        let fields = parameterized
            .body
            .iter()
            .map(|field| self.lower_field_declaration(&shape, field))
            .collect::<Result<Vec<(FieldLabel, TypeExpr)>, Diagnostic>>()?;
        core.items.push(Item {
            kind: ItemKind::Struct(binding, parameterized.binders, fields),
            span: form.span(),
            hidden: false,
        });
        Ok(())
    }

    fn lower_enum(
        &mut self,
        binding: BindingId,
        form: &Form,
        core: &mut CoreModule,
    ) -> Result<(), Diagnostic> {
        let shape = Shape::new(KernelForm::Definition(DefinitionForm::Enum), form);
        let items = shape.at_least(2)?;
        let first_variant = Self::first_enum_variant(items);
        let body = items[first_variant..].to_vec();
        if let Some(where_items) = self.old_parameters(&body) {
            let span = where_items[0].span();
            let kind = self
                .macro_origin_at(span)
                .map_or("enum", |origin| origin.name.as_ref());
            let replacement = self.old_head_replacement(kind, &items[1], where_items);
            return Err(ExpandProblems::parameters_on_name(kind, &replacement, span));
        }
        let enum_name = self.head_name(&shape, &items[1])?;
        let parameterized = self.lower_head(&shape, &items[1], body)?;
        let variants = parameterized
            .body
            .iter()
            .map(|variant| self.lower_variant(&shape, enum_name, variant))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        let rust =
            self.lower_enum_rust(&shape, items, &parameterized.binders, &variants, binding)?;
        core.items.push(Item {
            kind: ItemKind::Enum(binding, parameterized.binders, variants, rust),
            span: form.span(),
            hidden: false,
        });
        Ok(())
    }

    fn lower_enum_rust(
        &self,
        shape: &Shape,
        items: &[Form],
        binders: &[crate::core::TypeBinder],
        variants: &[VariantDeclaration],
        binding: BindingId,
    ) -> Result<Option<crate::core::EnumRust>, Diagnostic> {
        let Some(clause) = items
            .get(2)
            .filter(|clause| Self::starts_rust_clause(clause))
        else {
            return Ok(None);
        };
        let rust = Self::enum_rust(shape, clause, binders.len())?;
        if rust.template_clause {
            let template_form = clause
                .as_list()
                .and_then(|items| items.get(1))
                .expect("the rust clause holds a string");
            Self::check_type_placeholders(
                &rust.template,
                binders,
                self.bindings.get(binding).name(),
                template_form,
            )?;
            Self::check_enum_rust_template(
                items.get(1).expect("the enum has a head"),
                clause,
                variants,
                &rust,
                binders.len(),
            )?;
        }
        Ok(Some(rust))
    }

    fn check_enum_rust_template(
        enum_head: &Form,
        clause: &Form,
        variants: &[VariantDeclaration],
        rust: &crate::core::EnumRust,
        parameters: usize,
    ) -> Result<(), Diagnostic> {
        let example = Self::enum_rust_template_example(enum_head, variants, rust);
        let [variant] = variants else {
            return Err(ExpandProblems::enum_rust_template(
                clause.span(),
                "this template needs exactly one variant",
                &example,
            ));
        };
        let VariantPayload::Positional(parts) = &variant.payload else {
            return Err(ExpandProblems::enum_rust_template(
                clause.span(),
                "this template needs positional parts",
                &example,
            ));
        };
        if parts.len() != parameters {
            return Err(ExpandProblems::enum_rust_template(
                clause.span(),
                &format!(
                    "this template needs {}",
                    Counted::noun(parameters, "positional part")
                ),
                &example,
            ));
        }
        Ok(())
    }

    fn enum_rust_template_example(
        enum_head: &Form,
        variants: &[VariantDeclaration],
        rust: &crate::core::EnumRust,
    ) -> String {
        let variant = variants
            .first()
            .map_or("variant", |variant| variant.name.as_ref());
        let parameters = Self::enum_parameter_names(enum_head);
        let parts = parameters.join(" ");
        let variant = format!("({variant} {parts})");
        format!(
            "(enum {enum_head} (rust {:?}) {variant})",
            rust.path.as_ref()
        )
    }

    fn enum_parameter_names(enum_head: &Form) -> Vec<String> {
        enum_head
            .as_list()
            .and_then(|items| items.get(1..))
            .map(|parameters| parameters.iter().map(ToString::to_string).collect())
            .unwrap_or_default()
    }

    fn lower_variant(
        &mut self,
        shape: &Shape,
        enum_name: &Form,
        variant: &Form,
    ) -> Result<VariantDeclaration, Diagnostic> {
        let (variant_name, _) = self.variant_shape(shape, variant)?;
        let variant_symbol = variant_name
            .as_symbol()
            .ok_or_else(|| ExpandProblems::not_a_name(variant_name.span()))?;
        let binding = self.variant_binding(enum_name, variant_symbol, variant_name.span())?;
        let payload = self.lower_variant_payload(shape, variant)?;
        Ok(VariantDeclaration {
            binding,
            name: variant_symbol.name().into(),
            payload,
            span: variant.span(),
        })
    }

    fn variant_binding(
        &mut self,
        enum_name: &Form,
        variant: &Symbol,
        span: Span,
    ) -> Result<BindingId, Diagnostic> {
        let enum_symbol = enum_name
            .as_symbol()
            .ok_or_else(|| ExpandProblems::not_a_name(enum_name.span()))?;
        let constructor = Symbol::with_scopes(
            format!("{}/{}", enum_symbol.name(), variant.name()),
            enum_symbol.scopes().clone(),
        );
        self.resolve(&constructor, Namespace::Value, span)
    }

    fn lower_variant_payload(
        &mut self,
        shape: &Shape,
        variant: &Form,
    ) -> Result<VariantPayload, Diagnostic> {
        match variant.kind() {
            FormKind::Symbol(_) => Ok(VariantPayload::None),
            FormKind::List(items) => {
                let payload = &items[1..];
                if payload
                    .iter()
                    .any(|payload| self.is_struct_payload(payload))
                {
                    self.lower_named_payload(shape, variant, payload)
                } else if payload.is_empty() {
                    Ok(VariantPayload::None)
                } else {
                    payload
                        .iter()
                        .map(|payload| self.lower_type(payload))
                        .collect::<Result<Vec<_>, Diagnostic>>()
                        .map(VariantPayload::Positional)
                }
            }
            FormKind::Number(_) | FormKind::String(_) => Err(shape.malformed(variant)),
        }
    }

    fn lower_named_payload(
        &mut self,
        shape: &Shape,
        variant: &Form,
        payload: &[Form],
    ) -> Result<VariantPayload, Diagnostic> {
        let [single] = payload else {
            return Err(ExpandProblems::mixed_variant_payload(variant.span()));
        };
        let fields = single.as_list().map_or(&[][..], |fields| &fields[1..]);
        fields
            .iter()
            .map(|field| self.lower_field_declaration(shape, field))
            .collect::<Result<Vec<_>, Diagnostic>>()
            .map(VariantPayload::Named)
    }

    fn lower_derive(
        &mut self,
        module: ModuleId,
        form: &Form,
        core: &mut CoreModule,
    ) -> Result<(), Diagnostic> {
        let _ = module;
        let shape = Shape::new(KernelForm::Definition(DefinitionForm::Derive), form);
        let items = shape.at_least(2)?;
        let binding = self.resolve(shape.symbol(&items[1])?, Namespace::Type, items[1].span())?;
        if !matches!(
            self.kind(binding),
            BindingKind::Struct(owner) | BindingKind::Enum(owner) if owner == module
        ) {
            return Err(ExpandProblems::derive_target(
                self.bindings.get(binding).name(),
                items[1].span(),
            ));
        }
        let traits = items[2..]
            .iter()
            .map(|item| self.derived_trait(&shape, item))
            .collect::<Result<Vec<(BindingId, Span)>, Diagnostic>>()?;
        core.items.push(Item {
            kind: ItemKind::Derive(binding, items[1].span(), traits),
            span: form.span(),
            hidden: false,
        });
        Ok(())
    }

    fn derived_trait(
        &mut self,
        shape: &Shape,
        item: &Form,
    ) -> Result<(BindingId, Span), Diagnostic> {
        let binding = self.resolve(shape.symbol(item)?, Namespace::Type, item.span())?;
        if !matches!(self.kind(binding), BindingKind::Trait(_)) {
            return Err(ExpandProblems::derive_trait(
                self.bindings.get(binding).name(),
                item.span(),
            ));
        }
        Ok((binding, item.span()))
    }

    fn lower_field_declaration(
        &mut self,
        shape: &Shape,
        field: &Form,
    ) -> Result<(FieldLabel, TypeExpr), Diagnostic> {
        let [label, field_type] = shape.list(field)? else {
            return Err(shape.malformed(field));
        };
        Ok((
            self.field_label(shape, label)?,
            self.lower_type(field_type)?,
        ))
    }

    fn lower_trait(
        &mut self,
        binding: BindingId,
        form: &Form,
        core: &mut CoreModule,
    ) -> Result<(), Diagnostic> {
        let shape = Shape::new(KernelForm::Definition(DefinitionForm::Trait), form);
        let items = shape.at_least(2)?;
        let name = self.head_name(&shape, &items[1])?;
        let supertraits = self.lower_supertraits(&shape, &items[1])?;
        let rust_name: Rc<str> = Rc::from(NameMangler::type_name(name.to_string().as_str()));
        let scope = self.fresh_local_scope();
        let associated = self.define_associated_types(&shape, &items[1], &items[2..], scope)?;
        let mut self_symbol = Symbol::new("self");
        self_symbol.scopes_mut().add(scope);
        let self_form = Form::new(FormKind::Symbol(self_symbol), items[1].span());
        let self_variable = self.define_binder_variable(&self_form)?;
        let lowering = TraitLowering {
            shape: &shape,
            binding,
            self_variable,
            rust_name: &rust_name,
            scope,
            associated: &associated,
        };
        let method_forms: Vec<&Form> = items[2..]
            .iter()
            .filter(|entry| !Self::is_associated_entry(entry))
            .collect();
        let methods = self.lower_trait_methods(&lowering, &method_forms, core)?;
        let associated_core = associated
            .iter()
            .map(|(binding, name)| TraitAssociatedType {
                binding: *binding,
                name: Rc::clone(name),
            })
            .collect();
        self.trait_methods.insert(
            binding,
            TraitMethods {
                rust_name: Rc::clone(&rust_name),
                self_variable,
                associated: associated.clone(),
                methods: methods
                    .iter()
                    .map(|method| (method.binding, method.signature.clone()))
                    .collect(),
                defaults: methods
                    .iter()
                    .filter(|method| method.default.is_some())
                    .map(|method| method.binding)
                    .collect(),
            },
        );
        core.items.push(Item {
            kind: ItemKind::Trait(
                binding,
                rust_name,
                self_variable,
                supertraits,
                associated_core,
                methods,
            ),
            span: form.span(),
            hidden: false,
        });
        Ok(())
    }

    fn define_associated_types(
        &mut self,
        shape: &Shape<'_>,
        head: &Form,
        entries: &[Form],
        scope: crate::scope::Scope,
    ) -> Result<Vec<(BindingId, Rc<str>)>, Diagnostic> {
        let _ = head;
        entries
            .iter()
            .filter(|entry| Self::is_associated_entry(entry))
            .map(|entry| {
                let [_, name] = shape.list(entry)? else {
                    return Err(shape.malformed(entry));
                };
                let symbol = shape.symbol(name)?;
                let mut binder = name.clone();
                binder.add_scope(scope);
                let binding =
                    self.define_local(&binder, Namespace::Type, BindingKind::AssociatedType, None)?;
                Ok((binding, Rc::from(symbol.name())))
            })
            .collect()
    }

    fn is_associated_entry(entry: &Form) -> bool {
        entry
            .as_list()
            .and_then(|items| items.first())
            .and_then(Form::as_symbol)
            .is_some_and(|symbol| symbol.name() == "type")
    }

    fn lower_supertraits(
        &mut self,
        shape: &Shape<'_>,
        head: &Form,
    ) -> Result<Vec<TypeBound>, Diagnostic> {
        let Some([_, supertraits @ ..]) = head.as_list() else {
            return Ok(Vec::new());
        };
        supertraits
            .iter()
            .map(|supertrait| match supertrait.as_list() {
                Some([name]) => self.lower_bound(name),
                Some(_) => Err(shape
                    .malformed_with_help(supertrait, "write a trait name, such as `(display)`")),
                None => self.lower_bound(supertrait),
            })
            .collect()
    }

    fn lower_trait_methods(
        &mut self,
        lowering: &TraitLowering<'_, '_>,
        forms: &[&Form],
        core: &mut CoreModule,
    ) -> Result<Vec<TraitMethod>, Diagnostic> {
        let mut methods = Vec::new();
        for method in forms {
            let lowered = self.lower_trait_method(lowering, method, core);
            methods.push(lowered?);
        }
        Ok(methods)
    }

    fn lower_trait_method(
        &mut self,
        lowering: &TraitLowering<'_, '_>,
        method: &Form,
        core: &mut CoreModule,
    ) -> Result<TraitMethod, Diagnostic> {
        let method_items = method
            .as_list()
            .ok_or_else(|| lowering.shape.malformed(method))?;
        let method_name = &method_items[0];
        let signature_form = &method_items[1];
        let method_symbol = lowering.shape.symbol(method_name)?;
        let method_binding = self.resolve(method_symbol, Namespace::Value, method_name.span())?;
        let mut signature_form = signature_form.clone();
        signature_form.add_scope(lowering.scope);
        let signature = self.lower_type(&signature_form)?;
        let TypeExprKind::Function(_, parameters, _) = signature.kind() else {
            return Err(lowering.shape.malformed(&signature_form));
        };
        let scheme = Self::trait_method_scheme(
            lowering.self_variable,
            lowering.binding,
            lowering.associated,
            &signature,
            method.span(),
        );
        let template = RustTemplate {
            template: Rc::from(format!(
                "{}::{}",
                lowering.rust_name,
                NameMangler::value(method_symbol.name())
            )),
            type_expr: scheme.clone(),
        };
        let value = Expr::new(
            ExprKind::Leaf(Leaf::Rust(Rc::new(template))),
            signature_form.span(),
        );
        core.items.push(Item {
            kind: ItemKind::Def(method_binding, value),
            span: signature_form.span(),
            hidden: true,
        });
        let default = if method_items.len() > 2 {
            Some(self.lower_trait_default(
                lowering,
                parameters,
                method,
                &method_items[2..],
                &scheme,
                core,
            )?)
        } else {
            None
        };
        Ok(TraitMethod {
            binding: method_binding,
            signature,
            default,
        })
    }

    fn lower_trait_default(
        &mut self,
        lowering: &TraitLowering<'_, '_>,
        parameters: &[TypeExpr],
        method: &Form,
        body: &[Form],
        scheme: &TypeExpr,
        core: &mut CoreModule,
    ) -> Result<BindingId, Diagnostic> {
        let parameter_forms = parameters
            .iter()
            .enumerate()
            .map(|(position, parameter)| {
                let name = if position == 0 && Self::is_receiver(parameter, lowering.self_variable)
                {
                    "self".to_string()
                } else {
                    format!("argument-{position}")
                };
                Form::new(FormKind::Symbol(Symbol::new(name)), parameter.span())
            })
            .collect();
        let params = Form::new(FormKind::List(parameter_forms), method.span());
        let lambda = self.lower_lambda(lowering.shape, &params, body)?;
        let binding = self.bindings.add(BindingInfo::new(
            self.bindings.get(lowering.binding).name().into(),
            BindingKind::Local,
            Some(method.span()),
        ));
        core.items.push(Item {
            kind: ItemKind::Signature(binding, scheme.clone()),
            span: method.span(),
            hidden: true,
        });
        core.items.push(Item {
            kind: ItemKind::Def(
                binding,
                Expr::new(ExprKind::Fn(Rc::new(lambda)), method.span()),
            ),
            span: method.span(),
            hidden: true,
        });
        Ok(binding)
    }

    fn is_receiver(parameter: &TypeExpr, self_variable: BindingId) -> bool {
        match parameter.kind() {
            TypeExprKind::Named(binding, arguments) => {
                *binding == self_variable && arguments.is_empty()
            }
            TypeExprKind::Reference(_, _, inner) => Self::is_receiver(inner, self_variable),
            _ => false,
        }
    }

    fn trait_method_scheme(
        self_variable: BindingId,
        trait_binding: BindingId,
        associated: &[(BindingId, Rc<str>)],
        signature: &TypeExpr,
        span: Span,
    ) -> TypeExpr {
        let argument = associated
            .first()
            .map(|(binding, _)| TypeExpr::new(TypeExprKind::Named(*binding, Vec::new()), span));
        let mut binders = vec![TypeBinder {
            variable: self_variable,
            bounds: vec![TypeBound {
                binding: trait_binding,
                argument,
            }],
            span,
            lifetime: false,
        }];
        binders.extend(associated.iter().map(|(binding, _)| TypeBinder {
            variable: *binding,
            bounds: Vec::new(),
            span,
            lifetime: false,
        }));
        TypeExpr::new(
            TypeExprKind::Where(binders, Box::new(signature.clone())),
            span,
        )
    }

    fn lower_impl(
        &mut self,
        scope: crate::scope::Scope,
        form: &Form,
        core: &mut CoreModule,
    ) -> Result<(), Diagnostic> {
        let shape = Shape::new(KernelForm::Definition(DefinitionForm::Impl), form);
        let items = shape.at_least(3)?;
        let trait_binding = self.impl_trait(&shape, &items[1])?;
        let implementor = self.impl_type(&shape, &items[2], scope)?;
        let Some(trait_methods) = self.trait_methods.get(&trait_binding).cloned() else {
            return Err(shape.malformed_with_help(form, "define the trait before implementing it"));
        };
        let trait_name = self.bindings.get(trait_binding).name().to_string();
        let type_name = items[2].to_string();
        let (associated, method_forms) = self.lower_impl_associated(
            &shape,
            &items[3..],
            &trait_methods,
            &trait_name,
            &type_name,
            items[2].span(),
        )?;
        let lowering = ImplLowering {
            shape: &shape,
            trait_methods: &trait_methods,
            trait_name: &trait_name,
            implementor: &implementor,
            associated: &associated,
        };
        let (methods, implemented) = self.lower_impl_methods(&lowering, &method_forms, core)?;
        self.report_missing_methods(&lowering, &implemented, &type_name, items[2].span());
        let associated = associated
            .into_iter()
            .map(|(trait_type, name, value)| ImplAssociatedType {
                trait_type,
                name,
                value,
            })
            .collect();
        core.items.push(Item {
            kind: ItemKind::Impl(trait_binding, implementor, associated, methods),
            span: form.span(),
            hidden: false,
        });
        Ok(())
    }

    fn impl_trait(&mut self, shape: &Shape, form: &Form) -> Result<BindingId, Diagnostic> {
        let binding = self.resolve(shape.symbol(form)?, Namespace::Type, form.span())?;
        if matches!(self.kind(binding), BindingKind::Trait(_)) {
            return Ok(binding);
        }
        Err(shape.malformed_with_help(form, "write a visible trait"))
    }

    fn impl_type(
        &mut self,
        shape: &Shape,
        form: &Form,
        scope: crate::scope::Scope,
    ) -> Result<TypeExpr, Diagnostic> {
        self.reject_bare_implementor(form)?;
        let implementor = self.bind_implementor_variables(form, scope)?;
        if matches!(
            implementor.kind(),
            TypeExprKind::Named(..) | TypeExprKind::Primitive(_)
        ) {
            return Ok(implementor);
        }
        Err(shape.malformed_with_help(
            form,
            "write a concrete type such as `i64`, `string` or `(vec i64)`",
        ))
    }

    fn reject_bare_implementor(&self, form: &Form) -> Result<(), Diagnostic> {
        match form.as_symbol() {
            Some(symbol) if self.is_type_variable(symbol) => Err(
                ExpandProblems::generic_implementor(symbol.name(), form.span()),
            ),
            _ => Ok(()),
        }
    }

    fn bind_implementor_variables(
        &mut self,
        form: &Form,
        scope: crate::scope::Scope,
    ) -> Result<TypeExpr, Diagnostic> {
        let mut implementor = form.clone();
        let mut variables = self.unresolved_type_variables(&implementor);
        if variables.is_empty() {
            return self.lower_type(&implementor);
        }
        implementor.add_scope(scope);
        for variable in &mut variables {
            variable.add_scope(scope);
        }
        for variable in &variables {
            self.define_binder_variable(variable)?;
        }
        self.lower_type(&implementor)
    }

    fn unresolved_type_variables(&self, form: &Form) -> Vec<Form> {
        let mut found: Vec<Form> = Vec::new();
        self.collect_unresolved_type_variables(form, &mut found);
        found
    }

    fn collect_unresolved_type_variables(&self, form: &Form, found: &mut Vec<Form>) {
        if let FormKind::Symbol(symbol) = form.kind() {
            self.collect_unresolved_variable(form, symbol, found);
            return;
        }
        let Some(arguments) = self.type_arguments(form) else {
            return;
        };
        for argument in arguments {
            self.collect_unresolved_type_variables(argument, found);
        }
    }

    fn collect_unresolved_variable(&self, form: &Form, symbol: &Symbol, found: &mut Vec<Form>) {
        if self.is_type_variable(symbol) && !Self::already_found(found, symbol) {
            found.push(form.clone());
        }
    }

    fn already_found(found: &[Form], symbol: &Symbol) -> bool {
        found.iter().any(|existing| {
            existing
                .as_symbol()
                .is_some_and(|existing| existing.name() == symbol.name())
        })
    }

    fn type_arguments<'f>(&self, form: &'f Form) -> Option<&'f [Form]> {
        let items = form.as_list()?;
        let (head, arguments) = items.split_first()?;
        let symbol = head.as_symbol()?;
        self.try_resolve(symbol, Namespace::Type)?;
        Some(arguments)
    }

    fn is_type_variable(&self, symbol: &Symbol) -> bool {
        self.try_resolve(symbol, Namespace::Type).is_none()
            && self.try_resolve(symbol, Namespace::Value).is_none()
    }

    fn lower_impl_associated(
        &mut self,
        shape: &Shape<'_>,
        entries: &[Form],
        trait_methods: &TraitMethods,
        trait_name: &str,
        type_name: &str,
        implementor_span: Span,
    ) -> Result<ImplLoweringParts, Diagnostic> {
        let mut associated = Vec::new();
        let mut defined = std::collections::HashSet::new();
        let mut method_forms = Vec::new();
        for entry in entries {
            if !Self::is_associated_entry(entry) {
                method_forms.push(entry.clone());
                continue;
            }
            let Some((trait_type, name, value)) =
                self.lower_associated_entry(shape, entry, trait_methods, trait_name)?
            else {
                continue;
            };
            if !defined.insert(trait_type) {
                self.diagnostics
                    .push(ExpandProblems::duplicate_associated_type(
                        &name,
                        entry.span(),
                    ));
                continue;
            }
            associated.push((trait_type, name, value));
        }
        for (trait_type, name) in &trait_methods.associated {
            if !defined.contains(trait_type) {
                self.diagnostics
                    .push(ExpandProblems::missing_associated_type(
                        trait_name,
                        name,
                        type_name,
                        implementor_span,
                    ));
            }
        }
        Ok((associated, method_forms))
    }

    fn lower_associated_entry(
        &mut self,
        shape: &Shape<'_>,
        entry: &Form,
        trait_methods: &TraitMethods,
        trait_name: &str,
    ) -> Result<Option<(BindingId, Rc<str>, TypeExpr)>, Diagnostic> {
        let entry_items = entry.as_list().ok_or_else(|| shape.malformed(entry))?;
        let [_, name, value] = entry_items else {
            return Err(
                shape.malformed_with_help(entry, "write an associated type as `(type item a)`")
            );
        };
        let symbol = shape.symbol(name)?;
        let Some((trait_type, _)) = trait_methods
            .associated
            .iter()
            .find(|(_, declared)| declared.as_ref() == symbol.name())
        else {
            self.diagnostics
                .push(ExpandProblems::unknown_associated_type(
                    trait_name,
                    symbol.name(),
                    name.span(),
                ));
            return Ok(None);
        };
        let value = self.lower_type(value)?;
        Ok(Some((*trait_type, Rc::from(symbol.name()), value)))
    }

    fn lower_impl_methods(
        &mut self,
        lowering: &ImplLowering<'_, '_>,
        forms: &[Form],
        core: &mut CoreModule,
    ) -> Result<(Vec<ImplMethod>, std::collections::HashSet<BindingId>), Diagnostic> {
        let mut methods = Vec::new();
        let mut implemented = std::collections::HashSet::new();
        for method in forms {
            let lowered = self.lower_impl_method(lowering, method, &mut implemented, core);
            if let Some(method) = lowered? {
                methods.push(method);
            }
        }
        Ok((methods, implemented))
    }

    fn report_missing_methods(
        &mut self,
        lowering: &ImplLowering<'_, '_>,
        implemented: &std::collections::HashSet<BindingId>,
        type_name: &str,
        implementor_span: Span,
    ) {
        let trait_methods = lowering.trait_methods;
        for (method, _) in &trait_methods.methods {
            if !implemented.contains(method) && !trait_methods.defaults.contains(method) {
                let name = self.bindings.get(*method).name().to_string();
                self.diagnostics.push(ExpandProblems::missing_method(
                    &name,
                    lowering.trait_name,
                    type_name,
                    implementor_span,
                ));
            }
        }
    }

    fn lower_impl_method(
        &mut self,
        lowering: &ImplLowering<'_, '_>,
        method: &Form,
        implemented: &mut std::collections::HashSet<BindingId>,
        core: &mut CoreModule,
    ) -> Result<Option<ImplMethod>, Diagnostic> {
        let method_items = method
            .as_list()
            .ok_or_else(|| lowering.shape.malformed(method))?;
        let Some(method_name) = method_items.first() else {
            return Err(lowering.shape.malformed(method));
        };
        let Some(params) = method_items.get(1) else {
            return Err(lowering.shape.malformed(method));
        };
        let method_symbol = lowering.shape.symbol(method_name)?;
        let Some((method_binding, signature)) = lowering
            .trait_methods
            .methods
            .iter()
            .find(|(binding, _)| self.bindings.get(*binding).name() == method_symbol.name())
        else {
            self.diagnostics.push(ExpandProblems::undeclared_method(
                method_symbol.name(),
                lowering.trait_name,
                method_name.span(),
            ));
            return Ok(None);
        };
        if !implemented.insert(*method_binding) {
            self.diagnostics.push(ExpandProblems::extra_method(
                method_symbol.name(),
                method_name.span(),
            ));
            return Ok(None);
        }
        let specialized =
            signature.replacing(lowering.trait_methods.self_variable, lowering.implementor);
        let specialized = lowering
            .associated
            .iter()
            .fold(specialized, |signature, (binding, _, value)| {
                signature.replacing(*binding, value)
            });
        let specialized = Self::wrap_implementor_binders(
            &specialized,
            &self.implementor_variables(lowering.implementor),
        );
        let lambda = Rc::new(self.lower_lambda(lowering.shape, params, &method_items[2..])?);
        let synthetic = self.bindings.add(BindingInfo::new(
            self.bindings.get(*method_binding).name().into(),
            BindingKind::Local,
            Some(method_name.span()),
        ));
        core.items.push(Item {
            kind: ItemKind::Signature(synthetic, specialized),
            span: method_name.span(),
            hidden: true,
        });
        core.items.push(Item {
            kind: ItemKind::Def(
                synthetic,
                Expr::new(ExprKind::Fn(Rc::clone(&lambda)), method.span()),
            ),
            span: method.span(),
            hidden: true,
        });
        Ok(Some(ImplMethod {
            binding: synthetic,
            trait_method: *method_binding,
            name: self.bindings.get(*method_binding).name().into(),
            lambda,
        }))
    }

    fn implementor_variables(&self, implementor: &TypeExpr) -> Vec<BindingId> {
        let mut found = Vec::new();
        self.collect_implementor_variables(implementor, &mut found);
        found
    }

    fn collect_implementor_variables(&self, type_expr: &TypeExpr, found: &mut Vec<BindingId>) {
        if let TypeExprKind::Named(binding, arguments) = type_expr.kind()
            && arguments.is_empty()
            && matches!(self.kind(*binding), BindingKind::TypeVariable)
            && !found.contains(binding)
        {
            found.push(*binding);
        }
        for part in type_expr.parts() {
            self.collect_implementor_variables(part, found);
        }
    }

    fn wrap_implementor_binders(signature: &TypeExpr, variables: &[BindingId]) -> TypeExpr {
        if variables.is_empty() {
            return signature.clone();
        }
        let span = signature.span();
        let binders = variables
            .iter()
            .map(|variable| TypeBinder {
                variable: *variable,
                bounds: Vec::new(),
                span,
                lifetime: false,
            })
            .collect();
        TypeExpr::new(
            TypeExprKind::Where(binders, Box::new(signature.clone())),
            span,
        )
    }

    fn lower_rust_trait(
        &mut self,
        binding: BindingId,
        form: &Form,
        core: &mut CoreModule,
    ) -> Result<(), Diagnostic> {
        let shape = Shape::new(KernelForm::Definition(DefinitionForm::RustTrait), form);
        let items = shape.at_least(3)?;
        let path: Rc<str> = shape.string(&items[2])?.into();
        self.warn_second_trait_path(binding, &items[1], &path);
        let scope = self.fresh_local_scope();
        let mut self_symbol = Symbol::new("self");
        self_symbol.scopes_mut().add(scope);
        let self_form = Form::new(FormKind::Symbol(self_symbol), items[1].span());
        let self_variable = self.define_binder_variable(&self_form)?;
        let entries = self.lower_rust_trait_entries(&shape, scope, &items[3..])?;
        self.trait_methods.insert(
            binding,
            TraitMethods {
                rust_name: Rc::clone(&path),
                self_variable,
                associated: Vec::new(),
                methods: entries
                    .methods
                    .iter()
                    .map(|method| (method.binding, method.signature.clone()))
                    .collect(),
                defaults: std::collections::HashSet::new(),
            },
        );
        self.trait_paths.insert(binding, path.clone());
        core.items.push(Item {
            kind: ItemKind::RustTrait(RustTraitDeclaration {
                binding,
                path,
                self_variable,
                methods: entries.methods,
                help: entries.help,
                associated: entries.associated,
                implementors: entries.implementors,
            }),
            span: form.span(),
            hidden: false,
        });
        Ok(())
    }

    fn lower_rust_trait_entries(
        &mut self,
        shape: &Shape<'_>,
        scope: crate::scope::Scope,
        entries: &[Form],
    ) -> Result<RustTraitEntries, Diagnostic> {
        let mut lowered = RustTraitEntries {
            methods: Vec::new(),
            help: None,
            associated: Vec::new(),
            implementors: Vec::new(),
        };
        for entry in entries {
            self.lower_rust_trait_entry(shape, scope, entry, &mut lowered)?;
        }
        Ok(lowered)
    }

    fn lower_rust_trait_entry(
        &mut self,
        shape: &Shape<'_>,
        scope: crate::scope::Scope,
        entry: &Form,
        lowered: &mut RustTraitEntries,
    ) -> Result<(), Diagnostic> {
        if Self::is_rust_trait_method(entry) {
            lowered
                .methods
                .push(self.lower_rust_trait_method(shape, scope, entry)?);
        } else if Self::is_rust_trait_help(entry) {
            lowered.help = Some(self.lower_rust_trait_help(shape, entry)?);
        } else if Self::is_rust_trait_associated(entry) {
            lowered
                .associated
                .push(self.lower_rust_trait_associated(shape, entry)?);
        } else {
            lowered
                .implementors
                .push(self.lower_rust_trait_implementor(entry)?);
        }
        Ok(())
    }

    fn lower_rust_trait_associated(
        &mut self,
        shape: &Shape<'_>,
        entry: &Form,
    ) -> Result<RustTraitAssociatedType, Diagnostic> {
        let [_, head, rust_name] = shape.list(entry)? else {
            return Err(shape.malformed(entry));
        };
        let [name, parameter] = shape.list(head)? else {
            return Err(shape.malformed(head));
        };
        let symbol = shape.symbol(name)?;
        Ok(RustTraitAssociatedType {
            binding: self.resolve(symbol, Namespace::Type, name.span())?,
            parameter: shape.symbol(parameter)?.name().into(),
            rust_name: shape.string(rust_name)?.into(),
        })
    }

    fn lower_rust_trait_method(
        &mut self,
        shape: &Shape<'_>,
        scope: crate::scope::Scope,
        entry: &Form,
    ) -> Result<TraitMethod, Diagnostic> {
        let method = shape.list(entry)?;
        let [_, name, signature] = method else {
            return Err(shape.malformed(entry));
        };
        let symbol = shape.symbol(name)?;
        let binding = self.resolve(symbol, Namespace::Value, name.span())?;
        let mut lowered = signature.clone();
        lowered.add_scope(scope);
        let lowered = self.lower_type(&lowered)?;
        if !matches!(lowered.kind(), TypeExprKind::Function(..)) {
            return Err(shape.malformed(signature));
        }
        Ok(TraitMethod {
            binding,
            signature: lowered,
            default: None,
        })
    }

    fn lower_rust_trait_help(
        &mut self,
        shape: &Shape<'_>,
        entry: &Form,
    ) -> Result<Rc<str>, Diagnostic> {
        let help = shape.list(entry)?;
        let [_, text] = help else {
            return Err(shape.malformed(entry));
        };
        Ok(shape.string(text)?.into())
    }

    fn warn_second_trait_path(&mut self, binding: BindingId, name: &Form, path: &str) {
        let Some(symbol) = name.as_symbol() else {
            return;
        };
        let mut visible = self
            .table
            .visible_bindings(symbol.scopes(), Namespace::Type, self.phase);
        visible.sort();
        let existing = visible.into_iter().find(|existing| {
            *existing != binding
                && matches!(self.kind(*existing), BindingKind::Trait(_))
                && self
                    .trait_paths
                    .get(existing)
                    .is_some_and(|known| known.as_ref() == path)
        });
        let Some(existing) = existing else {
            return;
        };
        let span = name.span();
        let name = self.bindings.get(binding).name().to_string();
        let existing = self.bindings.get(existing).name().to_string();
        self.warnings.push(ExpandProblems::second_trait_path(
            &name, &existing, path, span,
        ));
    }

    fn lower_exports(&mut self, module: ModuleId, form: &Form, core: &mut CoreModule) {
        let shape = Shape::new(KernelForm::Definition(DefinitionForm::Export), form);
        for name in shape.items().iter().skip(1) {
            if let Err(diagnostic) = self.export_name(module, &shape, name, core) {
                self.diagnostics.push(diagnostic);
            }
        }
    }

    fn export_name(
        &mut self,
        module: ModuleId,
        shape: &Shape,
        name: &Form,
        core: &mut CoreModule,
    ) -> Result<(), Diagnostic> {
        let symbol = shape.symbol(name)?;
        let mut found = false;
        for namespace in [Namespace::Value, Namespace::Type] {
            if let Some(binding) = self.try_resolve(symbol, namespace) {
                self.modules
                    .get_mut(module)
                    .export(symbol.shared_name(), namespace, binding);
                core.exports.push(binding);
                found = true;
            }
        }
        if found {
            self.export_enum_constructors(module, symbol, core);
            self.export_trait_methods(module, symbol, core);
            Ok(())
        } else {
            self.resolve(symbol, Namespace::Value, name.span())
                .map(|_| ())
        }
    }

    fn export_trait_methods(&mut self, module: ModuleId, symbol: &Symbol, core: &mut CoreModule) {
        let Some(trait_binding) = self.try_resolve(symbol, Namespace::Type) else {
            return;
        };
        if !matches!(self.kind(trait_binding), BindingKind::Trait(_)) {
            return;
        }
        let Some(trait_methods) = self.trait_methods.get(&trait_binding) else {
            return;
        };
        let methods: Vec<BindingId> = trait_methods
            .methods
            .iter()
            .map(|(binding, _)| *binding)
            .collect();
        for method in methods {
            let name = self.bindings.get(method).name().to_string();
            self.modules
                .get_mut(module)
                .export(name.into(), Namespace::Value, method);
            core.exports.push(method);
        }
    }

    fn export_enum_constructors(
        &mut self,
        module: ModuleId,
        symbol: &Symbol,
        core: &mut CoreModule,
    ) {
        let Some(enum_binding) = self.try_resolve(symbol, Namespace::Type) else {
            return;
        };
        if !matches!(self.kind(enum_binding), BindingKind::Enum(_)) {
            return;
        }
        let Some(constructors) = self.enum_variants.get(&enum_binding).cloned() else {
            return;
        };
        for constructor in constructors {
            let name = self.bindings.get(constructor).name().to_string();
            self.modules
                .get_mut(module)
                .export(name.into(), Namespace::Value, constructor);
            core.exports.push(constructor);
        }
    }
}
