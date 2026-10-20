use crate::source::Span;

#[derive(Clone, Debug)]
pub struct Label {
    span: Span,
    message: String,
}

impl Label {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug)]
pub struct Suggestion {
    span: Span,
    replacement: String,
}

impl Suggestion {
    pub fn new(span: Span, replacement: impl Into<String>) -> Self {
        Self {
            span,
            replacement: replacement.into(),
        }
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn replacement(&self) -> &str {
        &self.replacement
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
