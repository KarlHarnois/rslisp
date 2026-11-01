use std::fmt;
use std::path::PathBuf;

use super::{Expander, ExpansionTables};
use crate::core::{ModuleId, ModulePath};
use crate::diagnostic::DiagnosticRenderer;
use crate::modules::{ModuleRecord, ModuleSource, ModuleSources};
use crate::source::{SourceId, SourceMap, Span};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct PreludeState {
    first_source: SourceId,
    prelude: ModuleId,
    modules: Vec<ModuleRecord>,
    tables: ExpansionTables,
}

#[derive(Debug)]
pub struct UnreadablePrelude(bincode::Error);

impl fmt::Display for UnreadablePrelude {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "the saved prelude state is unreadable: {}",
            self.0
        )
    }
}

impl PreludeState {
    const PLACEHOLDER_ENTRY: &str = "saved-prelude-entry";

    pub fn build() -> Self {
        let mut sources = SourceMap::default();
        let entry = sources.add(Self::PLACEHOLDER_ENTRY, "");
        let module_sources = ModuleSources::new(PathBuf::new(), PathBuf::new());
        Expander::new(&mut sources, module_sources).capture_prelude(entry)
    }

    pub fn save(&self) -> Vec<u8> {
        bincode::serialize(self).expect("every saved table serializes")
    }

    pub fn load(bytes: &[u8]) -> Result<Self, UnreadablePrelude> {
        bincode::deserialize(bytes).map_err(UnreadablePrelude)
    }

    pub(super) fn install(self, expander: &mut Expander<'_>) -> ModuleId {
        assert_eq!(
            expander.sources.next_id(),
            self.first_source,
            "the entry is the only source before the prelude"
        );
        for record in self.modules {
            expander.add_prelude_source(record.path());
            expander.modules.adopt(record);
        }
        expander.tables = self.tables;
        self.prelude
    }
}

impl Expander<'_> {
    fn capture_prelude(mut self, entry: SourceId) -> PreludeState {
        let placeholder =
            self.register_source_module(ModulePath::new(PreludeState::PLACEHOLDER_ENTRY));
        let first_source = self.sources.next_id();
        let prelude = self
            .load_module(
                &ModulePath::new(ModuleSources::PRELUDE),
                Span::new(entry, 0..0),
            )
            .expect("the prelude loads");
        self.assert_no_problems();
        PreludeState {
            first_source,
            prelude,
            modules: self.modules.into_records_after(placeholder),
            tables: self.tables,
        }
    }

    fn assert_no_problems(&mut self) {
        let mut problems = self.take_diagnostics();
        problems.extend(std::mem::take(&mut self.warnings));
        assert!(
            problems.is_empty(),
            "the prelude expands without problems, but it reports:\n{}",
            DiagnosticRenderer::plain(self.sources).render_all(&problems)
        );
    }

    fn add_prelude_source(&mut self, path: &ModulePath) {
        if let Some(ModuleSource::Text { name, text }) = self.module_sources.find(path) {
            self.sources.add(name, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct UserProgram {
        name: &'static str,
        text: &'static str,
    }

    impl UserProgram {
        const GARBLED_STATE: &[u8] = &[1, 2, 3];
        const MACROS_AND_NAMES: Self = Self {
            name: "user",
            text: "\
(defmacro define-caller (name function argument)
  (template (def (unquote name) (fn () ((unquote function) (unquote argument))))))
(def map (fn (x) (+ x 1)))
(define-caller call-it map 3)
(defn main () (println \"{}\" (call-it)))",
        };
        const NAMED_PRELUDE: Self = Self {
            name: "prelude",
            text: "(def answer 42)",
        };

        fn built_entry(&self) -> String {
            let (mut sources, entry) = self.sources();
            self.entry_module(Expander::new(&mut sources, Self::module_sources()), entry)
        }

        fn loaded_entry(&self) -> String {
            let (mut sources, entry) = self.sources();
            let expander =
                Expander::with_prelude(&mut sources, Self::module_sources(), Self::loaded_state());
            self.entry_module(expander, entry)
        }

        fn sources(&self) -> (SourceMap, SourceId) {
            let mut sources = SourceMap::default();
            let entry = sources.add(self.name, self.text);
            (sources, entry)
        }

        fn entry_module(&self, expander: Expander<'_>, entry: SourceId) -> String {
            let program = expander
                .expand(ModulePath::new(self.name), entry)
                .unwrap_or_else(|_| panic!("the user program expands"));
            format!("{:?}", program.entry_module())
        }

        fn loaded_state() -> PreludeState {
            PreludeState::load(&PreludeState::build().save()).expect("the saved prelude loads")
        }

        fn module_sources() -> ModuleSources {
            ModuleSources::new(PathBuf::new(), PathBuf::new())
        }
    }

    #[test]
    fn user_ids_continue_after_a_loaded_prelude() {
        let program = UserProgram::MACROS_AND_NAMES;
        assert_eq!(program.loaded_entry(), program.built_entry());
    }

    #[test]
    fn an_entry_named_prelude_expands_alike_on_a_loaded_state() {
        let program = UserProgram::NAMED_PRELUDE;
        assert_eq!(program.loaded_entry(), program.built_entry());
    }

    #[test]
    fn unreadable_bytes_report_an_unreadable_prelude() {
        let Err(problem) = PreludeState::load(UserProgram::GARBLED_STATE) else {
            panic!("garbled bytes do not load");
        };
        assert!(
            problem
                .to_string()
                .starts_with("the saved prelude state is unreadable: ")
        );
    }

    #[test]
    #[should_panic(expected = "the entry is the only source before the prelude")]
    fn a_source_before_the_entry_stops_the_install() {
        let program = UserProgram::MACROS_AND_NAMES;
        let mut sources = SourceMap::default();
        sources.add("earlier", "");
        let entry = sources.add(program.name, program.text);
        let expander = Expander::with_prelude(
            &mut sources,
            UserProgram::module_sources(),
            UserProgram::loaded_state(),
        );
        program.entry_module(expander, entry);
    }
}
