use std::ops::Range;

use rustc_hash::FxHashMap;
use serde_json::Value;

use crate::diagnostic::{Diagnostic, DiagnosticCode, Diagnostics, Label};
use crate::emit::EmittedCrate;
use crate::source::Span;

pub struct RustcDiagnosticMapper<'a> {
    emitted: &'a EmittedCrate,
    fallback: Span,
    user_names: FxHashMap<&'a str, &'a str>,
}

impl<'a> RustcDiagnosticMapper<'a> {
    const ERROR_LEVEL: &'static str = "error";
    const SUMMARY_PREFIX: &'static str = "aborting due to";
    const REPORTED_BY_RUSTC: &'static str = "reported by rustc while compiling the generated Rust";

    pub fn new(emitted: &'a EmittedCrate, fallback: Span) -> Self {
        let user_names = emitted
            .names
            .iter()
            .map(|(rust, user)| (rust.as_str(), user.as_str()))
            .collect();
        Self {
            emitted,
            fallback,
            user_names,
        }
    }

    pub fn map(&self, stderr: &str) -> Diagnostics {
        let mut diagnostics = Diagnostics::default();
        for message in stderr
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        {
            if let Some(diagnostic) = self.diagnostic(&message) {
                diagnostics.push(diagnostic);
            }
        }
        if !diagnostics.has_errors() {
            diagnostics.push(self.unexplained(stderr));
        }
        diagnostics
    }

    fn diagnostic(&self, message: &Value) -> Option<Diagnostic> {
        let text = message["message"].as_str()?;
        if message["level"].as_str() != Some(Self::ERROR_LEVEL)
            || text.starts_with(Self::SUMMARY_PREFIX)
        {
            return None;
        }
        let code = message["code"]["code"]
            .as_str()
            .map_or(DiagnosticCode::RUST_ERROR, |code| {
                DiagnosticCode::from_rustc(code.to_string())
            });
        let (span, label) = self.primary_span(message);
        let diagnostic = Diagnostic::error(
            code,
            self.user_text(text),
            Label::new(span, self.user_text(&label)),
        );
        Some(
            self.with_children(diagnostic, message)
                .with_note(Self::REPORTED_BY_RUSTC),
        )
    }

    fn primary_span(&self, message: &Value) -> (Span, String) {
        let spans = message["spans"].as_array().cloned().unwrap_or_default();
        let primary = spans
            .iter()
            .find(|span| span["is_primary"].as_bool() == Some(true));
        let Some(primary) = primary else {
            return (self.fallback, "in the generated program".to_string());
        };
        let label = primary["label"].as_str().unwrap_or("here").to_string();
        let range = Self::byte_range(primary);
        let span = range
            .and_then(|range| self.emitted.span_map.source_of(range))
            .unwrap_or(self.fallback);
        (span, label)
    }

    fn byte_range(span: &Value) -> Option<Range<usize>> {
        let start = usize::try_from(span["byte_start"].as_u64()?).ok()?;
        let end = usize::try_from(span["byte_end"].as_u64()?).ok()?;
        Some(start..end)
    }

    fn with_children(&self, diagnostic: Diagnostic, message: &Value) -> Diagnostic {
        let children = message["children"].as_array().cloned().unwrap_or_default();
        children.iter().fold(diagnostic, |diagnostic, child| {
            let Some(text) = child["message"].as_str() else {
                return diagnostic;
            };
            match child["level"].as_str() {
                Some("help") => diagnostic.with_help(self.user_text(text)),
                _ => diagnostic.with_note(self.user_text(text)),
            }
        })
    }

    fn unexplained(&self, stderr: &str) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::RUST_ERROR,
            "rustc could not compile the generated program",
            Label::new(self.fallback, "while compiling this program"),
        )
        .with_note(stderr.trim().to_string())
    }

    fn user_text(&self, text: &str) -> String {
        let mut rewritten = String::new();
        for (position, part) in text.split('`').enumerate() {
            if position > 0 {
                rewritten.push('`');
            }
            let is_code = position % 2 == 1;
            rewritten.push_str(&if is_code {
                self.user_code(part)
            } else {
                part.to_string()
            });
        }
        rewritten
    }

    fn user_code(&self, code: &str) -> String {
        let mut rewritten = String::new();
        let mut word = String::new();
        for character in code.chars().chain(std::iter::once(' ')) {
            if character.is_alphanumeric() || character == '_' {
                word.push(character);
                continue;
            }
            rewritten.push_str(self.user_names.get(word.as_str()).copied().unwrap_or(&word));
            word.clear();
            rewritten.push(character);
        }
        rewritten.pop();
        rewritten
    }
}
