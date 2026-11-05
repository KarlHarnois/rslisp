mod block;
mod document;
mod import_order;
mod layout;
mod list_plan;
mod meaning;
mod measure;
mod shape_reader;
mod shapes;
mod syntax_tree;

pub use shape_reader::ModuleLookup;
pub use shapes::{Shape, ShapeCatalog, ShapeStatement};

use crate::diagnostic::Diagnostics;
use crate::reader::{Form, Reader};
use crate::source::{SourceFile, SourceMap};
use document::DocumentPrinter;
use import_order::ImportSorter;
use layout::LayoutEngine;
use meaning::MeaningComparison;
use shape_reader::ShapeReader;
use syntax_tree::SyntaxTree;

#[derive(Default)]
pub struct SourceFormatter {
    shape_reader: ShapeReader,
}

#[derive(Debug)]
pub enum FormatFailure {
    Unreadable(Diagnostics),
    ChangedMeaning,
    Unstable,
}

struct FormattedText<'s> {
    shapes: &'s ShapeCatalog,
    text: String,
}

impl SourceFormatter {
    const FORMATTED_SOURCE: &'static str = "<formatted>";

    pub fn format(
        &mut self,
        source: &SourceFile,
        lookup: &ModuleLookup,
    ) -> Result<String, FormatFailure> {
        let written = Reader::new(source)
            .read()
            .map_err(FormatFailure::Unreadable)?;
        let shapes = self.shape_reader.catalog(&written, lookup);
        let formatted = FormattedText::lay_out(&shapes, source.text());
        formatted.verify(&written, source.text())?;
        Ok(formatted.text)
    }
}

impl<'s> FormattedText<'s> {
    fn lay_out(shapes: &'s ShapeCatalog, text: &str) -> Self {
        let mut tree = SyntaxTree::build(text);
        ImportSorter::sort(&mut tree);
        let mut engine = LayoutEngine::new(shapes);
        let text = DocumentPrinter::new(&mut engine).print(&tree);
        Self { shapes, text }
    }

    fn verify(&self, written: &[Form], written_text: &str) -> Result<(), FormatFailure> {
        let comparison = MeaningComparison::new(self.shapes);
        let keeps_meaning = comparison.same_comments(written_text, &self.text)
            && self
                .read()
                .is_some_and(|formatted| comparison.same_forms(written, &formatted));
        if !keeps_meaning {
            return Err(FormatFailure::ChangedMeaning);
        }
        if Self::lay_out(self.shapes, &self.text).text != self.text {
            return Err(FormatFailure::Unstable);
        }
        Ok(())
    }

    fn read(&self) -> Option<Vec<Form>> {
        let mut sources = SourceMap::default();
        let source = sources.add(SourceFormatter::FORMATTED_SOURCE, self.text.as_str());
        Reader::new(sources.file(source)).read().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::{FormatFailure, FormattedText, ShapeCatalog, ShapeStatement};
    use crate::reader::{Form, Reader};
    use crate::source::SourceMap;

    struct CheckFixture {
        shapes: ShapeCatalog,
    }

    impl CheckFixture {
        fn with_docstring_shape() -> Self {
            let mut shapes = ShapeCatalog::default();
            let statement = ShapeStatement::of(
                &Self::forms("(header-shape (defn name parameters) :docstring)")[0],
            )
            .expect("the statement states a shape");
            shapes.add(&statement);
            Self { shapes }
        }

        fn verify(&self, written: &str, formatted: &str) -> Result<(), FormatFailure> {
            FormattedText {
                shapes: &self.shapes,
                text: formatted.to_string(),
            }
            .verify(&Self::forms(written), written)
        }

        fn forms(text: &str) -> Vec<Form> {
            let mut sources = SourceMap::default();
            let source = sources.add("written", text);
            Reader::new(sources.file(source))
                .read()
                .unwrap_or_else(|_| panic!("the text reads"))
        }
    }

    #[test]
    fn text_that_reads_to_other_forms_changes_the_meaning() {
        let fixture = CheckFixture::with_docstring_shape();
        assert!(matches!(
            fixture.verify("(add 1 2)", "(add 1 3)\n"),
            Err(FormatFailure::ChangedMeaning)
        ));
    }

    #[test]
    fn text_that_lays_out_differently_again_is_unstable() {
        let fixture = CheckFixture::with_docstring_shape();
        assert!(matches!(
            fixture.verify("(add 1 2)", "(add 1\n     2)\n"),
            Err(FormatFailure::Unstable)
        ));
    }

    #[test]
    fn a_moved_docstring_line_keeps_the_meaning() {
        let fixture = CheckFixture::with_docstring_shape();
        let written = "(defn f ()\n      \"Doc.\n      More.\"\n      1)";
        let formatted = "(defn f ()\n  \"Doc.\n  More.\"\n  1)\n";
        assert!(fixture.verify(written, formatted).is_ok());
    }

    #[test]
    fn a_moved_line_of_a_string_that_is_no_docstring_changes_the_meaning() {
        let fixture = CheckFixture::with_docstring_shape();
        let written = "(def f\n      \"Text.\n      More.\")";
        let formatted = "(def f\n  \"Text.\n  More.\")\n";
        assert!(matches!(
            fixture.verify(written, formatted),
            Err(FormatFailure::ChangedMeaning)
        ));
    }

    #[test]
    fn a_number_keeps_its_written_text() {
        let fixture = CheckFixture::with_docstring_shape();
        assert!(matches!(
            fixture.verify("(add 1_000)", "(add 1000)\n"),
            Err(FormatFailure::ChangedMeaning)
        ));
    }

    #[test]
    fn text_that_drops_a_comment_changes_the_meaning() {
        let fixture = CheckFixture::with_docstring_shape();
        assert!(matches!(
            fixture.verify("(f :key ; why\n   value)", "(f :key value)\n"),
            Err(FormatFailure::ChangedMeaning)
        ));
    }

    #[test]
    fn a_moved_line_of_a_last_string_in_docstring_position_changes_the_meaning() {
        let fixture = CheckFixture::with_docstring_shape();
        let written = "(defn f ()\n      \"Text.\n      More.\")";
        let formatted = "(defn f ()\n  \"Text.\n  More.\")\n";
        assert!(matches!(
            fixture.verify(written, formatted),
            Err(FormatFailure::ChangedMeaning)
        ));
    }

    #[test]
    fn text_that_merges_two_comments_changes_the_meaning() {
        let fixture = CheckFixture::with_docstring_shape();
        assert!(matches!(
            fixture.verify("(f (g a ; c1\n) ; c2\n b)", "(f (g a) ; c1 ; c2\n   b)\n"),
            Err(FormatFailure::ChangedMeaning)
        ));
    }

    #[test]
    fn sorted_import_names_keep_the_meaning() {
        let fixture = CheckFixture::with_docstring_shape();
        assert!(
            fixture
                .verify("(use m b a)\n(export d c)", "(use m a b)\n\n(export c d)\n")
                .is_ok()
        );
    }

    #[test]
    fn an_import_list_with_another_name_changes_the_meaning() {
        let fixture = CheckFixture::with_docstring_shape();
        assert!(matches!(
            fixture.verify("(use m b a)", "(use m a c)\n"),
            Err(FormatFailure::ChangedMeaning)
        ));
    }

    #[test]
    fn an_import_from_another_module_changes_the_meaning() {
        let fixture = CheckFixture::with_docstring_shape();
        assert!(matches!(
            fixture.verify("(use m b a)", "(use n a b)\n"),
            Err(FormatFailure::ChangedMeaning)
        ));
    }

    #[test]
    fn a_reordered_call_changes_the_meaning() {
        let fixture = CheckFixture::with_docstring_shape();
        assert!(matches!(
            fixture.verify("(f b a)", "(f a b)\n"),
            Err(FormatFailure::ChangedMeaning)
        ));
    }

    #[test]
    fn a_list_that_loses_a_form_changes_the_meaning() {
        let fixture = CheckFixture::with_docstring_shape();
        assert!(matches!(
            fixture.verify("(f a b)", "(f a)\n"),
            Err(FormatFailure::ChangedMeaning)
        ));
    }

    #[test]
    fn a_reordered_quoted_import_changes_the_meaning() {
        let fixture = CheckFixture::with_docstring_shape();
        assert!(matches!(
            fixture.verify(
                "(def x (quote (use m b a)))",
                "(def x (quote (use m a b)))\n"
            ),
            Err(FormatFailure::ChangedMeaning)
        ));
    }

    #[test]
    fn an_import_that_loses_a_name_changes_the_meaning() {
        let fixture = CheckFixture::with_docstring_shape();
        assert!(matches!(
            fixture.verify("(use m a b)", "(use m a)\n"),
            Err(FormatFailure::ChangedMeaning)
        ));
    }
}
