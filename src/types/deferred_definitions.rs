use rustc_hash::FxHashSet;

use crate::core::BindingId;
use crate::expand::Program;
use crate::modules::ModuleSources;

pub struct DeferredDefinitions;

impl DeferredDefinitions {
    pub fn of(program: &Program) -> FxHashSet<BindingId> {
        let always_checked = Self::always_checked(program);
        program
            .modules
            .iter()
            .filter(|module| module.id != program.entry)
            .filter(|module| ModuleSources::is_embedded_prelude(&module.path))
            .flat_map(|module| module.definitions())
            .map(|(binding, _)| binding)
            .filter(|binding| !always_checked.contains(binding))
            .chain(program.pending_definitions.undecoded())
            .collect()
    }

    fn always_checked(program: &Program) -> FxHashSet<BindingId> {
        let methods = program
            .modules
            .iter()
            .flat_map(|module| &module.items)
            .flat_map(|item| item.kind.method_bodies());
        methods
            .chain(program.string_view)
            .chain(program.checked_roots.iter().copied())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::DeferredDefinitions;
    use crate::core::{BindingId, CoreModule, ModulePath};
    use crate::expand::Program;
    use crate::modules::ModuleSources;
    use crate::types::TypedSource;

    struct PreludeModuleFixture {
        source: TypedSource,
    }

    impl PreludeModuleFixture {
        const MODULE: &str = "
            (trait named
              (name (fn ((ref self)) i64) (self) 1)
              (rank (fn ((ref self)) i64)))
            (defstruct car :year i64)
            (impl named car
              (rank (c) (:year c)))
            (defn helper () 1)
            (defn main ())";
        const METHOD_BODIES: usize = 2;

        fn new() -> Self {
            Self::moved_to(ModuleSources::PRELUDE)
        }

        fn moved_to(path: &str) -> Self {
            let mut source = TypedSource::expand(Self::MODULE);
            source.move_entry_to(ModulePath::new(path));
            source.detach_entry();
            Self { source }
        }

        fn program(&self) -> &Program {
            self.source.program()
        }

        fn module(&self) -> &CoreModule {
            self.program()
                .modules
                .last()
                .expect("the module is expanded")
        }

        fn definition(&self, name: &str) -> BindingId {
            let bindings = &self.program().bindings;
            self.module()
                .definitions()
                .map(|(binding, _)| binding)
                .find(|binding| bindings.get(*binding).name() == name)
                .expect("the module defines the name")
        }

        fn defers(&self, name: &str) -> bool {
            self.defers_binding(self.definition(name))
        }

        fn defers_binding(&self, binding: BindingId) -> bool {
            DeferredDefinitions::of(self.program()).contains(&binding)
        }

        fn methods(&self) -> Vec<BindingId> {
            self.module()
                .items
                .iter()
                .flat_map(|item| item.kind.method_bodies())
                .collect()
        }
    }

    #[test]
    fn a_prelude_function_waits_for_a_reference() {
        assert!(PreludeModuleFixture::new().defers("helper"));
    }

    #[test]
    fn a_user_module_under_the_prelude_directory_never_waits() {
        assert!(!PreludeModuleFixture::moved_to("prelude/extra").defers("helper"));
    }

    #[test]
    fn an_entry_named_like_the_prelude_never_waits() {
        let mut source = TypedSource::expand(PreludeModuleFixture::MODULE);
        source.move_entry_to(ModulePath::new(ModuleSources::PRELUDE));
        let fixture = PreludeModuleFixture { source };
        assert!(!fixture.defers("helper"));
    }

    #[test]
    fn a_written_definition_never_waits() {
        let source = TypedSource::expand("(defn helper () 1) (defn main ())");
        let program = source.program();
        let helper = program
            .entry_module()
            .definitions()
            .map(|(binding, _)| binding)
            .next()
            .expect("the module defines a function");
        assert!(!DeferredDefinitions::of(program).contains(&helper));
    }

    #[test]
    fn every_method_body_never_waits() {
        let fixture = PreludeModuleFixture::new();
        let methods = fixture.methods();
        assert_eq!(methods.len(), PreludeModuleFixture::METHOD_BODIES);
        assert!(!methods.iter().any(|method| fixture.defers_binding(*method)));
    }

    #[test]
    fn a_checked_root_never_waits() {
        let mut fixture = PreludeModuleFixture::new();
        let helper = fixture.definition("helper");
        fixture.source.check_also(helper);
        assert!(!fixture.defers("helper"));
    }

    #[test]
    fn the_string_view_never_waits() {
        let source = TypedSource::expand("(defn main ())");
        let program = source.program();
        let view = program.string_view.expect("the prelude views strings");
        assert!(!DeferredDefinitions::of(program).contains(&view));
    }
}
