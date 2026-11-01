use std::fs;
use std::path::PathBuf;

use syn::{Expr, ImplItem, Item, Lit, Type};

use crate::failure::TaskFailure;
use crate::workspace::CargoWorkspace;

pub struct LanguageSource {
    path: PathBuf,
}

impl LanguageSource {
    const FILE: &'static str = "language.rs";
    const TYPE: &'static str = "Language";

    pub fn find(workspace: &CargoWorkspace) -> Result<Self, TaskFailure> {
        workspace
            .source_directories()
            .into_iter()
            .map(|directory| directory.join(Self::FILE))
            .find(|path| path.is_file())
            .map(|path| Self { path })
            .ok_or_else(|| TaskFailure::language_missing(Self::FILE))
    }

    pub fn constant(&self, name: &str) -> Result<String, TaskFailure> {
        let text = fs::read_to_string(&self.path)
            .map_err(|error| TaskFailure::unreadable(&self.path, error))?;
        let syntax =
            syn::parse_file(&text).map_err(|error| TaskFailure::unparsable(&self.path, &error))?;
        Self::constant_in(&syntax, name)
            .ok_or_else(|| TaskFailure::constant_missing(&self.path, Self::TYPE, name))
    }

    fn constant_in(syntax: &syn::File, name: &str) -> Option<String> {
        syntax
            .items
            .iter()
            .filter_map(Self::language_impl)
            .flat_map(|block| &block.items)
            .find_map(|item| Self::string_constant(item, name))
    }

    fn language_impl(item: &Item) -> Option<&syn::ItemImpl> {
        let Item::Impl(block) = item else {
            return None;
        };
        let Type::Path(self_type) = block.self_ty.as_ref() else {
            return None;
        };
        self_type.path.is_ident(Self::TYPE).then_some(block)
    }

    fn string_constant(item: &ImplItem, name: &str) -> Option<String> {
        let ImplItem::Const(constant) = item else {
            return None;
        };
        let Expr::Lit(literal) = &constant.expr else {
            return None;
        };
        let Lit::Str(text) = &literal.lit else {
            return None;
        };
        (constant.ident == name).then(|| text.value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::WorkspaceFixture;

    const SOURCE: &str = r#"
        pub struct Language;

        impl Other {
            pub const EXTENSION: &str = "wrong";
        }

        impl Language {
            pub const NAME: &str = "fakelang";
            pub const EXTENSION: &str = "fake";
        }
    "#;

    #[test]
    fn reads_a_string_constant_of_the_language_type() {
        let syntax = syn::parse_file(SOURCE).unwrap();
        assert_eq!(
            LanguageSource::constant_in(&syntax, "EXTENSION").as_deref(),
            Some("fake")
        );
        assert_eq!(LanguageSource::constant_in(&syntax, "VERSION"), None);
    }

    #[test]
    fn finds_the_language_file_in_whichever_crate_holds_it() {
        let language = LanguageSource::find(&WorkspaceFixture::workspace()).unwrap();
        assert_eq!(language.constant("EXTENSION").unwrap(), "ldg");
    }

    #[test]
    fn reports_a_workspace_whose_crates_hold_no_language_file() {
        let failure = LanguageSource::find(&WorkspaceFixture::without_crates()).err();
        assert!(failure.unwrap().to_string().contains("`language.rs`"));
    }
}
