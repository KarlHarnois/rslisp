use std::path::PathBuf;

use crate::core::ModulePath;
use crate::language::Language;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinModule {
    Kernel,
    Code,
}

pub enum ModuleSource {
    Builtin(BuiltinModule),
    Text { name: String, text: String },
}

pub struct ModuleSources {
    root: PathBuf,
}

impl ModuleSources {
    pub const KERNEL: &str = "kernel";
    pub const CODE: &str = "code";
    pub const PRELUDE: &str = "prelude";
    const PRELUDE_DIRECTORY: &str = "prelude/";
    const PRELUDE_FILES: [(&str, &str); 4] = [
        ("prelude", include_str!("../../prelude/prelude.rslisp")),
        (
            "prelude/macros",
            include_str!("../../prelude/macros.rslisp"),
        ),
        ("prelude/logic", include_str!("../../prelude/logic.rslisp")),
        (
            "prelude/syntax",
            include_str!("../../prelude/syntax.rslisp"),
        ),
    ];

    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn is_prelude(path: &ModulePath) -> bool {
        path.as_str() == Self::PRELUDE || path.as_str().starts_with(Self::PRELUDE_DIRECTORY)
    }

    pub fn find(&self, path: &ModulePath) -> Option<ModuleSource> {
        self.builtin(path).or_else(|| self.prelude_file(path))
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    fn builtin(&self, path: &ModulePath) -> Option<ModuleSource> {
        match path.as_str() {
            Self::KERNEL => Some(ModuleSource::Builtin(BuiltinModule::Kernel)),
            Self::CODE => Some(ModuleSource::Builtin(BuiltinModule::Code)),
            _ => None,
        }
    }

    fn prelude_file(&self, path: &ModulePath) -> Option<ModuleSource> {
        Self::PRELUDE_FILES
            .iter()
            .find(|(module, _)| *module == path.as_str())
            .map(|(module, text)| ModuleSource::Text {
                name: format!("{module}.{}", Language::EXTENSION),
                text: (*text).to_string(),
            })
    }
}
