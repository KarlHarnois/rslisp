use rustc_hash::FxHashMap;

use crate::core::Keyword;
use crate::reader::Form;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    pub header_length: usize,
    pub takes_docstring: bool,
    pub one_per_line: bool,
    pub arms: bool,
    pub bindings: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShapeStatement {
    pub form_name: String,
    pub shape: Shape,
}

#[derive(Clone, Default)]
pub struct ShapeCatalog {
    shapes: FxHashMap<String, Shape>,
}

impl ShapeStatement {
    pub const STATING_FORM: &'static str = "header-shape";
    pub const DOCSTRING_MARKER: &'static str = "docstring";
    pub const ONE_PER_LINE_MARKER: &'static str = "one-per-line";
    pub const ARMS_MARKER: &'static str = "arms";
    pub const BINDINGS_MARKER: &'static str = "bindings";

    pub fn of(form: &Form) -> Option<Self> {
        let [head, stated, markers @ ..] = form.as_list()? else {
            return None;
        };
        (head.as_symbol()?.name() == Self::STATING_FORM).then_some(())?;
        let [form_name, header @ ..] = stated.as_list()? else {
            return None;
        };
        Some(Self {
            form_name: form_name.as_symbol()?.name().to_string(),
            shape: Shape {
                header_length: header.len(),
                takes_docstring: Self::has_marker(markers, Self::DOCSTRING_MARKER),
                one_per_line: Self::has_marker(markers, Self::ONE_PER_LINE_MARKER),
                arms: Self::has_marker(markers, Self::ARMS_MARKER),
                bindings: Self::has_marker(markers, Self::BINDINGS_MARKER),
            },
        })
    }

    fn has_marker(markers: &[Form], marker: &str) -> bool {
        markers.iter().any(|form| {
            form.as_symbol()
                .and_then(|symbol| Keyword::field_name(symbol.name()))
                == Some(marker)
        })
    }
}

impl ShapeCatalog {
    pub fn add(&mut self, statement: &ShapeStatement) {
        self.shapes
            .insert(statement.form_name.clone(), statement.shape);
    }

    pub fn of(&self, form_name: &str) -> Option<Shape> {
        self.shapes.get(form_name).copied()
    }

    pub fn docstring_position(&self, form_name: &str) -> Option<usize> {
        self.of(form_name)
            .filter(|shape| shape.takes_docstring)
            .map(|shape| shape.header_length + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::{Shape, ShapeStatement};
    use crate::reader::{Form, Reader};
    use crate::source::SourceMap;

    struct StatementFixture;

    impl StatementFixture {
        fn statement(text: &str) -> Option<ShapeStatement> {
            ShapeStatement::of(&Self::form(text))
        }

        fn form(text: &str) -> Form {
            let mut sources = SourceMap::default();
            let source = sources.add("shape", text);
            Reader::new(sources.file(source))
                .read()
                .unwrap_or_else(|_| panic!("the statement reads"))
                .remove(0)
        }
    }

    #[test]
    fn a_statement_counts_the_forms_after_the_form_name() {
        let statement = StatementFixture::statement("(header-shape (impl trait type))");
        assert_eq!(
            statement.map(|statement| statement.shape),
            Some(Shape {
                header_length: 2,
                takes_docstring: false,
                one_per_line: false,
                arms: false,
                bindings: false
            })
        );
    }

    #[test]
    fn a_marker_after_the_shape_states_a_docstring() {
        let statement =
            StatementFixture::statement("(header-shape (defn name parameters) :docstring)");
        assert!(statement.is_some_and(|statement| statement.shape.takes_docstring));
    }

    #[test]
    fn a_one_per_line_marker_states_no_docstring() {
        let statement = StatementFixture::statement("(header-shape (enum name) :one-per-line)");
        assert_eq!(
            statement.map(|statement| statement.shape),
            Some(Shape {
                header_length: 1,
                takes_docstring: false,
                one_per_line: true,
                arms: false,
                bindings: false
            })
        );
    }

    #[test]
    fn an_arms_marker_states_arms_alone() {
        let statement = StatementFixture::statement("(header-shape (match value) :arms)");
        assert_eq!(
            statement.map(|statement| statement.shape),
            Some(Shape {
                header_length: 1,
                takes_docstring: false,
                one_per_line: false,
                arms: true,
                bindings: false
            })
        );
    }

    #[test]
    fn a_bindings_marker_states_a_binding_list() {
        let statement = StatementFixture::statement("(header-shape (let bindings) :bindings)");
        assert_eq!(
            statement.map(|statement| statement.shape),
            Some(Shape {
                header_length: 1,
                takes_docstring: false,
                one_per_line: false,
                arms: false,
                bindings: true
            })
        );
    }

    #[test]
    fn a_call_of_another_form_states_no_shape() {
        assert_eq!(StatementFixture::statement("(document (defn name))"), None);
    }

    #[test]
    fn a_shape_that_does_not_start_with_a_name_states_nothing() {
        assert_eq!(
            StatementFixture::statement("(header-shape (\"defn\" name))"),
            None
        );
    }
}
