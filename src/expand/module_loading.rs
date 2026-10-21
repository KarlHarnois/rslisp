use std::rc::Rc;

use super::Expander;
use super::context::DefinitionContext;
use super::problems::ExpandProblems;
use super::shape::Shape;
use crate::core::{
    BindingInfo, BindingKind, Builtin, DefinitionForm, KernelForm, KernelType, ModuleId,
    ModulePath, Namespace,
};
use crate::diagnostic::Diagnostic;
use crate::diagnostic::SimilarName;
use crate::modules::{BuiltinModule, ModuleSource, ModuleSources};
use crate::reader::{Form, Reader, Symbol};
use crate::scope::{Phase, ScopeSet};
use crate::source::{SourceId, Span};

impl Expander<'_> {
    const NAMESPACES: [Namespace; 2] = [Namespace::Value, Namespace::Type];

    pub(super) fn load_module(
        &mut self,
        path: &ModulePath,
        span: Span,
    ) -> Result<ModuleId, Diagnostic> {
        if let Some(id) = self.modules.find(path) {
            return self.loaded(id, path, span);
        }
        match self.module_sources.find(path) {
            Some(ModuleSource::Builtin(builtin)) => Ok(self.create_builtin(path, builtin)),
            Some(ModuleSource::Text { name, text }) => {
                let source = self.sources.add(name, text);
                Ok(self.load_source_module(path.clone(), source))
            }
            None => Err(ExpandProblems::unknown_module(path, span)),
        }
    }

    pub(super) fn load_source_module(&mut self, path: ModulePath, source: SourceId) -> ModuleId {
        let base_scope = self.scopes.fresh();
        let module_scope = self.scopes.fresh();
        let id = self.modules.register(path.clone(), base_scope);
        let forms = self.read_module(source);
        self.import_base(id, &path, Span::new(source, 0..0));
        let scoped = forms
            .into_iter()
            .map(|mut form| {
                form.add_scope(base_scope);
                form.add_scope(module_scope);
                form
            })
            .collect();
        self.expand_module_body(id, scoped);
        self.modules.finish_loading(id);
        id
    }

    pub(super) fn import(
        &mut self,
        form: &Form,
        context: &DefinitionContext,
    ) -> Result<(), Diagnostic> {
        let shape = Shape::new(KernelForm::Definition(DefinitionForm::Use), form);
        let items = shape.at_least(2)?;
        let path = ModulePath::new(shape.symbol(&items[1])?.name());
        let target = self.load_module(&path, items[1].span())?;
        for name in &items[2..] {
            let mut binder = name.clone();
            context.strip_use_sites(&mut binder);
            let symbol = binder.as_symbol().ok_or_else(|| shape.malformed(name))?;
            if let Err(diagnostic) = self.import_name(target, &path, symbol, name.span()) {
                self.diagnostics.push(diagnostic);
            }
        }
        Ok(())
    }

    fn loaded(&self, id: ModuleId, path: &ModulePath, span: Span) -> Result<ModuleId, Diagnostic> {
        if !self.modules.get(id).is_loading() {
            return Ok(id);
        }
        let mut cycle: Vec<String> = self
            .modules
            .loading_paths()
            .iter()
            .map(ToString::to_string)
            .collect();
        cycle.push(path.to_string());
        Err(ExpandProblems::import_cycle(path, span, cycle))
    }

    fn read_module(&mut self, source: SourceId) -> Vec<Form> {
        match Reader::new(self.sources.file(source)).read() {
            Ok(forms) => forms,
            Err(diagnostics) => {
                self.diagnostics.extend(diagnostics);
                Vec::new()
            }
        }
    }

    fn import_base(&mut self, module: ModuleId, path: &ModulePath, span: Span) {
        let base_name = if ModuleSources::is_prelude(path) {
            ModuleSources::KERNEL
        } else {
            ModuleSources::PRELUDE
        };
        match self.load_module(&ModulePath::new(base_name), span) {
            Ok(base) => self.import_everything(module, base),
            Err(diagnostic) => self.diagnostics.push(diagnostic),
        }
    }

    fn import_everything(&mut self, module: ModuleId, base: ModuleId) {
        let mut scopes = ScopeSet::default();
        scopes.add(self.modules.get(module).base_scope());
        let exports: Vec<(Rc<str>, Namespace, _)> = self
            .modules
            .get(base)
            .exports()
            .map(|(name, namespace, binding)| (Rc::clone(name), namespace, binding))
            .collect();
        for (name, namespace, binding) in exports {
            let symbol = Symbol::with_scopes(name, scopes.clone());
            self.table
                .bind(&symbol, namespace, binding, Phase::Runtime)
                .expect("a module's exports have distinct names");
        }
    }

    fn import_name(
        &mut self,
        target: ModuleId,
        path: &ModulePath,
        symbol: &Symbol,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let mut found = false;
        for namespace in Self::NAMESPACES {
            if let Some(binding) = self.modules.get(target).exported(symbol.name(), namespace) {
                self.bind_import(symbol, namespace, binding, span)?;
                found = true;
            }
        }
        if found {
            Ok(())
        } else {
            Err(self.missing_export(target, path, symbol, span))
        }
    }

    fn missing_export(
        &self,
        target: ModuleId,
        path: &ModulePath,
        symbol: &Symbol,
        span: Span,
    ) -> Diagnostic {
        let exported: Vec<Rc<str>> = self
            .modules
            .get(target)
            .exports()
            .map(|(name, _, _)| Rc::clone(name))
            .collect();
        let similar = SimilarName::closest(symbol.name(), &exported);
        ExpandProblems::missing_export(path, symbol.name(), span, similar)
    }

    fn create_builtin(&mut self, path: &ModulePath, builtin: BuiltinModule) -> ModuleId {
        let id = self.modules.register(path.clone(), self.scopes.fresh());
        for (name, namespace, kind) in Self::builtin_exports(builtin) {
            let binding = self.bindings.add(BindingInfo::new(name.into(), kind, None));
            self.modules
                .get_mut(id)
                .export(name.into(), namespace, binding);
        }
        self.modules.finish_loading(id);
        id
    }

    fn builtin_exports(builtin: BuiltinModule) -> Vec<(&'static str, Namespace, BindingKind)> {
        match builtin {
            BuiltinModule::Kernel => KernelForm::all()
                .map(|form| (form.name(), Namespace::Value, BindingKind::KernelForm(form)))
                .chain(KernelType::all().map(|kernel_type| {
                    (
                        kernel_type.name(),
                        Namespace::Type,
                        BindingKind::KernelType(kernel_type),
                    )
                }))
                .collect(),
            BuiltinModule::Code => Builtin::all()
                .map(|builtin| {
                    (
                        builtin.name(),
                        Namespace::Value,
                        BindingKind::CodeBuiltin(builtin),
                    )
                })
                .collect(),
        }
    }
}
