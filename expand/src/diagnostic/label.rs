use crate::source::Span;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Label {
    span: Span,
    message: String,
    marks_a_macro_call: bool,
}

impl Label {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            marks_a_macro_call: false,
        }
    }

    pub fn at_macro_call(span: Span, message: impl Into<String>) -> Self {
        Self {
            marks_a_macro_call: true,
            ..Self::new(span, message)
        }
    }

    pub fn marks_a_macro_call(&self) -> bool {
        self.marks_a_macro_call
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug)]
pub struct Edit {
    span: Span,
    replacement: String,
}

impl Edit {
    pub fn span(&self) -> Span {
        self.span
    }

    pub fn replacement(&self) -> &str {
        &self.replacement
    }
}

#[derive(Clone, Debug)]
pub struct Suggestion {
    edits: Vec<Edit>,
}

impl Suggestion {
    pub fn new(span: Span, replacement: impl Into<String>) -> Self {
        Self {
            edits: vec![Edit {
                span,
                replacement: replacement.into(),
            }],
        }
    }

    pub fn wrapping(span: Span, opening: impl Into<String>, closing: impl Into<String>) -> Self {
        Self {
            edits: vec![
                Edit {
                    span: span.start(),
                    replacement: opening.into(),
                },
                Edit {
                    span: span.end(),
                    replacement: closing.into(),
                },
            ],
        }
    }

    pub fn edits(&self) -> &[Edit] {
        &self.edits
    }
}

#[derive(Clone, Debug)]
pub struct Help {
    message: String,
    suggestion: Option<Suggestion>,
}

impl Help {
    pub fn new(message: String) -> Self {
        Self {
            message,
            suggestion: None,
        }
    }

    pub fn with_suggestion(self, suggestion: Suggestion) -> Self {
        Self {
            suggestion: Some(suggestion),
            ..self
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn suggestion(&self) -> Option<&Suggestion> {
        self.suggestion.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::Label;
    use crate::source::{SourceId, Span};

    struct LabelFixture;

    impl LabelFixture {
        fn span() -> Span {
            Span::new(SourceId::new(0), 0..1)
        }
    }

    #[test]
    fn a_label_at_a_macro_call_marks_the_call() {
        assert!(Label::at_macro_call(LabelFixture::span(), "call").marks_a_macro_call());
    }

    #[test]
    fn a_plain_label_marks_no_macro_call() {
        assert!(!Label::new(LabelFixture::span(), "plain").marks_a_macro_call());
    }
}
