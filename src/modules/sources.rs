use std::fs;
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
    display_root: PathBuf,
}

impl ModuleSources {
    pub const KERNEL: &str = "kernel";
    pub const CODE: &str = "code";
    pub const PRELUDE: &str = "prelude";
    const PRELUDE_DIRECTORY: &str = "prelude/";
    const PRELUDE_FILES: [(&str, &str); 13] = [
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
        (
            "prelude/number",
            include_str!("../../prelude/number.rslisp"),
        ),
        ("prelude/text", include_str!("../../prelude/text.rslisp")),
        (
            "prelude/struct",
            include_str!("../../prelude/struct.rslisp"),
        ),
        (
            "prelude/ownership",
            include_str!("../../prelude/ownership.rslisp"),
        ),
        ("prelude/types", include_str!("../../prelude/types.rslisp")),
        ("prelude/vec", include_str!("../../prelude/vec.rslisp")),
        (
            "prelude/iterator",
            include_str!("../../prelude/iterator.rslisp"),
        ),
        (
            "prelude/option",
            include_str!("../../prelude/option.rslisp"),
        ),
        (
            "prelude/result",
            include_str!("../../prelude/result.rslisp"),
        ),
    ];

    pub fn new(root: PathBuf, display_root: PathBuf) -> Self {
        Self { root, display_root }
    }

    pub fn is_prelude(path: &ModulePath) -> bool {
        path.as_str() == Self::PRELUDE || path.as_str().starts_with(Self::PRELUDE_DIRECTORY)
    }

    pub fn find(&self, path: &ModulePath) -> Option<ModuleSource> {
        self.builtin(path)
            .or_else(|| self.prelude_file(path))
            .or_else(|| self.user_file(path))
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
            .map(|(_, text)| ModuleSource::Text {
                name: Self::file_name(path),
                text: (*text).to_string(),
            })
    }

    fn user_file(&self, path: &ModulePath) -> Option<ModuleSource> {
        let file_name = Self::file_name(path);
        let text = fs::read_to_string(self.root.join(&file_name)).ok()?;
        Some(ModuleSource::Text {
            name: self.display_root.join(&file_name).display().to_string(),
            text,
        })
    }

    fn file_name(path: &ModulePath) -> String {
        format!("{path}.{}", Language::EXTENSION)
    }
}
