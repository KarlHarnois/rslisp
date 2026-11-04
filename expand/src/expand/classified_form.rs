use super::Expander;
use super::evaluated_forms::{BoundName, EvaluatedForms};
use crate::core::ModulePath;
use crate::source::{SourceId, Span};

#[derive(Debug, PartialEq, Eq)]
pub enum ClassifiedForm {
    Expression,
    Definition(Vec<BoundName>),
}

impl Expander<'_> {
    pub fn classify(
        mut self,
        path: ModulePath,
        sources: &[SourceId],
        form: Span,
    ) -> ClassifiedForm {
        self.evaluated_forms = EvaluatedForms::new([form]);
        self.load_source_module(path, sources);
        if self.evaluated_forms.is_definition(form) {
            ClassifiedForm::Definition(self.evaluated_forms.bound_by(form))
        } else {
            ClassifiedForm::Expression
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::core::Namespace;
    use crate::modules::ModuleSources;
    use crate::source::SourceMap;

    struct InputFixture {
        sources: SourceMap,
        earlier: Vec<SourceId>,
    }

    impl InputFixture {
        const MODULE: &str = "prompt";

        fn new(earlier: &[&str]) -> Self {
            let mut sources = SourceMap::default();
            let earlier = earlier
                .iter()
                .map(|text| sources.add(Self::MODULE, *text))
                .collect();
            Self { sources, earlier }
        }

        fn classify(&mut self, input: &str) -> ClassifiedForm {
            let source = self.sources.add(Self::MODULE, input);
            let mut module = self.earlier.clone();
            module.push(source);
            Expander::new(
                &mut self.sources,
                ModuleSources::new(PathBuf::new(), PathBuf::new()),
            )
            .classify(
                ModulePath::new(Self::MODULE),
                &module,
                Span::new(source, 0..input.len()),
            )
        }

        fn names(&mut self, input: &str) -> Vec<(String, Namespace)> {
            let ClassifiedForm::Definition(names) = self.classify(input) else {
                panic!("`{input}` is a definition");
            };
            names
                .into_iter()
                .map(|bound| (bound.name.to_string(), bound.namespace))
                .collect()
        }
    }

    #[test]
    fn a_call_is_an_expression() {
        let mut fixture = InputFixture::new(&["(defn twice (x) (* 2 x))"]);
        assert_eq!(fixture.classify("(twice 3)"), ClassifiedForm::Expression);
    }

    #[test]
    fn a_function_binds_its_name() {
        let mut fixture = InputFixture::new(&[]);
        assert_eq!(
            fixture.names("(defn twice (x) (* 2 x))"),
            [("twice".to_string(), Namespace::Value)]
        );
    }

    #[test]
    fn a_struct_binds_its_type_and_its_constructor_but_no_introduced_name() {
        let mut fixture = InputFixture::new(&[]);
        assert_eq!(
            fixture.names("(defstruct point :x i64 :y i64)"),
            [
                ("point".to_string(), Namespace::Type),
                ("->point".to_string(), Namespace::Value)
            ]
        );
    }

    #[test]
    fn a_definition_of_an_earlier_name_still_binds_the_name() {
        let mut fixture = InputFixture::new(&["(defn twice (x) (* 2 x))"]);
        assert_eq!(
            fixture.names("(defn twice (x) (+ x x))"),
            [("twice".to_string(), Namespace::Value)]
        );
    }

    #[test]
    fn a_definition_binds_no_name_of_an_earlier_form() {
        let mut fixture = InputFixture::new(&["(defn twice (x) (* 2 x))"]);
        assert_eq!(
            fixture.names("(defn thrice (x) (* 3 x))"),
            [("thrice".to_string(), Namespace::Value)]
        );
    }
}
