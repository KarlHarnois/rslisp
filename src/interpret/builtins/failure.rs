use crate::core::Keyword;
use crate::diagnostic::{Diagnostic, DiagnosticCode, SimilarName, Suggestion};
use crate::interpret::arguments::Arguments;
use crate::interpret::problems::InterpretProblems;
use crate::reader::Form;
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
    earlier: Option<Span>,
    replacement: Option<String>,
}

impl FailureDescription {
    pub const PARTS: [&'static str; 3] = [Self::CODE, Self::EARLIER, Self::SUGGEST];
    const CODE: &'static str = "code";
    const EARLIER: &'static str = "earlier";
    const SUGGEST: &'static str = "suggest";

    fn read(parts: &[Form]) -> Result<Self, Diagnostic> {
        let mut pairs = parts.chunks_exact(2);
        let lone_part = pairs.remainder().first();
        let description = pairs.try_fold(Self::default(), |description, pair| {
            description.with_part(&pair[0], &pair[1])
        })?;
        match lone_part {
            Some(part) => Err(InterpretProblems::malformed_failure_part(part.span())),
            None => Ok(description),
        }
    }

    fn with_part(self, part: &Form, value: &Form) -> Result<Self, Diagnostic> {
        match Self::part_name(part) {
            Some(Self::CODE) => Ok(Self {
                code: Self::code(value)?,
                ..self
            }),
            Some(Self::EARLIER) => Ok(Self {
                earlier: Some(value.span()),
                ..self
            }),
            Some(Self::SUGGEST) => Ok(Self {
                replacement: Some(Self::symbol_name(value)?.to_string()),
                ..self
            }),
            _ => Err(InterpretProblems::malformed_failure_part(part.span())),
        }
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
        match self.earlier {
            Some(earlier) => InterpretProblems::failed_again(self.code, message, culprit, earlier),
            None => InterpretProblems::failed(self.code, message, culprit),
        }
    }
}

impl Default for FailureDescription {
    fn default() -> Self {
        Self {
            code: DiagnosticCode::MACRO_FAILED,
            earlier: None,
            replacement: None,
        }
    }
}
