use crate::core::Keyword;
use crate::diagnostic::{Diagnostic, DiagnosticCode, SimilarName, Suggestion};
use crate::interpret::arguments::Arguments;
use crate::interpret::problems::InterpretProblems;
use crate::reader::{Form, FormKind};
use crate::source::Span;

pub struct FailCall<'a> {
    arguments: &'a Arguments,
}

impl<'a> FailCall<'a> {
    const MESSAGE: usize = 0;
    const CULPRIT: usize = 1;
    const HELP: usize = 2;
    const DESCRIPTION: usize = 3;
    const FEWEST_ARGUMENTS: usize = 2;
    const MOST_ARGUMENTS: usize = 4;

    pub fn new(arguments: &'a Arguments) -> Self {
        Self { arguments }
    }

    pub fn failure(&self) -> Diagnostic {
        match self.diagnostic() {
            Ok(failure) | Err(failure) => failure,
        }
    }

    fn diagnostic(&self) -> Result<Diagnostic, Diagnostic> {
        self.expect_count_in_range()?;
        let message = self.arguments.string(Self::MESSAGE)?.to_string();
        let culprit = self.arguments.form(Self::CULPRIT)?.span();
        let help = self.help()?;
        Ok(self.description()?.diagnostic(message, culprit, help))
    }

    fn expect_count_in_range(&self) -> Result<(), Diagnostic> {
        let count = self.arguments.values().len();
        self.arguments
            .expect_count(count.clamp(Self::FEWEST_ARGUMENTS, Self::MOST_ARGUMENTS))
    }

    fn description(&self) -> Result<FailureDescription, Diagnostic> {
        if self.has_argument(Self::DESCRIPTION) {
            FailureDescription::read(self.arguments.list(Self::DESCRIPTION)?)
        } else {
            Ok(FailureDescription::default())
        }
    }

    fn help(&self) -> Result<Option<String>, Diagnostic> {
        if self.has_argument(Self::HELP) {
            Ok(Some(self.arguments.string(Self::HELP)?.to_string()))
        } else {
            Ok(None)
        }
    }

    fn has_argument(&self, index: usize) -> bool {
        index < self.arguments.values().len()
    }
}

pub struct FailureDescription {
    code: DiagnosticCode,
    label: Option<String>,
    earlier: Option<Span>,
    earlier_label: Option<String>,
    replacement: Option<String>,
}

impl FailureDescription {
    pub const PARTS: [&'static str; 5] = [
        Self::CODE,
        Self::EARLIER,
        Self::EARLIER_LABEL,
        Self::LABEL,
        Self::SUGGEST,
    ];
    const CODE: &'static str = "code";
    const DEFAULT_LABEL: &'static str = "here";
    const EARLIER: &'static str = "earlier";
    const EARLIER_LABEL: &'static str = "earlier-label";
    const LABEL: &'static str = "label";
    const SUGGEST: &'static str = "suggest";

    fn read(parts: &[Form]) -> Result<Self, Diagnostic> {
        let mut pairs = parts.chunks_exact(2);
        let lone_part = pairs.remainder().first();
        let description = pairs.try_fold(Self::default(), |description, pair| {
            description.with_part(&pair[0], &pair[1])
        })?;
        match lone_part {
            Some(part) => Err(InterpretProblems::malformed_failure_part(part.span())),
            None => description.expect_earlier_label(),
        }
    }

    fn expect_earlier_label(self) -> Result<Self, Diagnostic> {
        match (self.earlier, self.earlier_label.is_some()) {
            (Some(earlier), false) => Err(InterpretProblems::missing_earlier_label(earlier)),
            _ => Ok(self),
        }
    }

    fn with_part(self, part: &Form, value: &Form) -> Result<Self, Diagnostic> {
        match Self::part_name(part) {
            Some(Self::CODE) => self.with_code(value),
            Some(Self::EARLIER) => Ok(self.with_earlier(value)),
            Some(Self::EARLIER_LABEL) => self.with_earlier_label(value),
            Some(Self::LABEL) => self.with_label(value),
            Some(Self::SUGGEST) => self.with_replacement(value),
            _ => Err(InterpretProblems::malformed_failure_part(part.span())),
        }
    }

    fn with_code(self, value: &Form) -> Result<Self, Diagnostic> {
        Ok(Self {
            code: Self::code(value)?,
            ..self
        })
    }

    fn with_earlier(self, value: &Form) -> Self {
        Self {
            earlier: Some(value.span()),
            ..self
        }
    }

    fn with_earlier_label(self, value: &Form) -> Result<Self, Diagnostic> {
        Ok(Self {
            earlier_label: Some(Self::string(value)?.to_owned()),
            ..self
        })
    }

    fn with_label(self, value: &Form) -> Result<Self, Diagnostic> {
        Ok(Self {
            label: Some(Self::string(value)?.to_owned()),
            ..self
        })
    }

    fn with_replacement(self, value: &Form) -> Result<Self, Diagnostic> {
        Ok(Self {
            replacement: Some(Self::symbol_name(value)?.to_string()),
            ..self
        })
    }

    fn part_name(part: &Form) -> Option<&str> {
        part.as_symbol()
            .and_then(|symbol| Keyword::field_name(symbol.name()))
    }

    fn code(value: &Form) -> Result<DiagnosticCode, Diagnostic> {
        let name = Self::symbol_name(value)?;
        DiagnosticCode::macro_stage_named(name).ok_or_else(|| {
            let similar = SimilarName::closest(name, &DiagnosticCode::macro_stage_names());
            InterpretProblems::unknown_failure_code(name, value.span(), similar)
        })
    }

    fn symbol_name(value: &Form) -> Result<&str, Diagnostic> {
        value
            .as_symbol()
            .map(|symbol| symbol.name())
            .ok_or_else(|| InterpretProblems::malformed_failure_part(value.span()))
    }

    fn string(value: &Form) -> Result<&str, Diagnostic> {
        match value.kind() {
            FormKind::String(text) => Ok(text),
            _ => Err(InterpretProblems::malformed_failure_part(value.span())),
        }
    }

    fn diagnostic(mut self, message: String, culprit: Span, help: Option<String>) -> Diagnostic {
        let replacement = self.replacement.take();
        let failure = self.labelled(message, culprit);
        match (help, replacement) {
            (Some(help), Some(replacement)) => {
                failure.with_suggestion(help, Suggestion::new(culprit, replacement))
            }
            (Some(help), None) => failure.with_help(help),
            (None, _) => failure,
        }
    }

    fn labelled(self, message: String, culprit: Span) -> Diagnostic {
        let label = self.label.unwrap_or_else(|| Self::DEFAULT_LABEL.to_owned());
        let earlier = self.earlier.zip(self.earlier_label);
        InterpretProblems::failed(self.code, message, culprit, label, earlier)
    }
}

impl Default for FailureDescription {
    fn default() -> Self {
        Self {
            code: DiagnosticCode::MACRO_FAILED,
            label: None,
            earlier: None,
            earlier_label: None,
            replacement: None,
        }
    }
}
