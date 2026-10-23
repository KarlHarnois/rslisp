use std::collections::VecDeque;
use std::rc::Rc;

use super::Expander;
use super::context::DefinitionContext;
use super::problems::ExpandProblems;
use super::shape::Shape;
use crate::core::{
    BindingId, BindingKind, ControlForm, CoreModule, DefinitionForm, ExpressionForm, FieldLabel,
    Item, ItemKind, KernelForm, ModuleId, Namespace, TypeExpr, VariantDeclaration, VariantInfo,
    VariantPayload,
};
use crate::diagnostic::Diagnostic;
use crate::reader::{Form, FormKind, Symbol};
use crate::source::Span;

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
        for item in pending {
            if let Err(diagnostic) = self.lower_pending(module, item, &mut core) {
                self.diagnostics.push(diagnostic);
            }
        }
        self.compile_pending_macros();
        self.order.push(module);
        self.expanded.insert(module, core);
        self.current_module = outer_module;
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
            match self.register_top_level(module, form, &mut context, &mut queue) {
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
        let mut constructors = Vec::new();
        for (index, variant) in items[2..].iter().enumerate() {
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
        let binding = self.register_type_name(
            &form,
            KernelForm::Definition(DefinitionForm::RustTrait),
            BindingKind::Trait(module),
            context,
        )?;
        Ok(Some(PendingItem::RustTrait(binding, form)))
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
            });
        }
        let value = self.lowered_global(binding, span)?;
        core.items.push(Item {
            kind: ItemKind::Def(binding, (*value).clone()),
            span,
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
        let binding = self.resolve(shape.symbol(&items[1])?, Namespace::Value, items[1].span())?;
        if self.kind(binding) != BindingKind::Global(module) {
            return Err(ExpandProblems::signature_target(
                self.bindings.get(binding).name(),
                items[1].span(),
            ));
        }
        let signature = self.lower_type(&items[2])?;
        core.items.push(Item {
            kind: ItemKind::Signature(binding, signature),
            span: form.span(),
        });
        Ok(())
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
        let body = items[2..].to_vec();
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
        core.items.push(Item {
            kind: ItemKind::Enum(binding, parameterized.binders, variants),
            span: form.span(),
        });
        Ok(())
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
        if self.kind(binding) != BindingKind::Struct(module) {
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
            kind: ItemKind::Derive(binding, traits),
            span: form.span(),
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
        let implementors = items[3..]
            .iter()
            .map(|item| self.lower_type(item))
            .collect::<Result<Vec<TypeExpr>, Diagnostic>>()?;
        self.trait_paths.insert(binding, path.clone());
        core.items.push(Item {
            kind: ItemKind::RustTrait(binding, path, implementors),
            span: form.span(),
        });
        Ok(())
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
            Ok(())
        } else {
            self.resolve(symbol, Namespace::Value, name.span())
                .map(|_| ())
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
