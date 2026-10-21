use super::fragment::{Code, Fragment, Precedence};
use crate::core::{ParsedTemplate, Piece};

#[derive(Debug, PartialEq, Eq)]
pub enum TemplateError {
    MalformedPlaceholder,
    MissingArgument(usize),
}

pub struct Template {
    parsed: ParsedTemplate,
}

impl Template {
    const TEMPORARY_PREFIX: &str = "template_argument_";
    const UNARY_OPERATORS: [char; 4] = ['-', '!', '*', '&'];
    const POSTFIX_OPERATORS: [char; 4] = ['.', '[', '(', '?'];
    const OPENERS: [char; 3] = ['(', '[', '{'];
    const CLOSERS: [char; 3] = [')', ']', '}'];
    const SEPARATORS_BEFORE: [char; 2] = ['(', ','];
    const SEPARATORS_AFTER: [char; 2] = [',', ')'];

    pub fn parse(text: &str) -> Result<Self, TemplateError> {
        ParsedTemplate::parse(text)
            .map(|parsed| Self { parsed })
            .map_err(|_| TemplateError::MalformedPlaceholder)
    }

    pub fn is_path(&self) -> bool {
        self.parsed.is_path()
    }

    pub fn inline(&self, arguments: Vec<Code>) -> Result<Code, TemplateError> {
        if let Some(missing) = self
            .parsed
            .placeholders()
            .find(|index| *index >= arguments.len())
        {
            return Err(TemplateError::MissingArgument(missing));
        }
        if let [Piece::Placeholder(index)] = self.parsed.pieces() {
            return Ok(arguments[*index].clone());
        }
        if self.needs_temporaries(&arguments) {
            return Ok(self.inline_with_temporaries(arguments));
        }
        Ok(self.substitute(&arguments))
    }

    fn substitute(&self, arguments: &[Code]) -> Code {
        let mut fragments = Vec::new();
        for (position, piece) in self.parsed.pieces().iter().enumerate() {
            match piece {
                Piece::Literal(literal) => fragments.push(Fragment::Text(literal.clone())),
                Piece::Placeholder(index) => fragments.extend(
                    self.argument_in_place(&arguments[*index], position)
                        .fragments,
                ),
            }
        }
        Code {
            fragments,
            precedence: self.precedence(),
        }
    }

    fn argument_in_place(&self, argument: &Code, position: usize) -> Code {
        if argument.is_atomic()
            || self.is_isolated(position)
            || self.holds_prefix(argument, position)
        {
            argument.clone()
        } else {
            argument.clone().parenthesized()
        }
    }

    fn holds_prefix(&self, argument: &Code, position: usize) -> bool {
        let followed_by_postfix = self
            .literal_at(position + 1)
            .is_some_and(|text| text.starts_with(Self::POSTFIX_OPERATORS));
        argument.precedence == Precedence::Prefix && !followed_by_postfix
    }

    fn is_isolated(&self, position: usize) -> bool {
        let before = position
            .checked_sub(1)
            .and_then(|previous| self.literal_at(previous))
            .and_then(|text| text.trim_end().chars().last());
        let after = self
            .literal_at(position + 1)
            .and_then(|text| text.trim_start().chars().next());
        before.is_some_and(|character| Self::SEPARATORS_BEFORE.contains(&character))
            && after.is_some_and(|character| Self::SEPARATORS_AFTER.contains(&character))
    }

    fn literal_at(&self, position: usize) -> Option<&str> {
        match self.parsed.pieces().get(position)? {
            Piece::Literal(literal) => Some(literal),
            Piece::Placeholder(_) => None,
        }
    }

    fn precedence(&self) -> Precedence {
        let text = self.parsed.text().trim();
        if text.starts_with(Self::UNARY_OPERATORS) {
            Precedence::Prefix
        } else if Self::has_top_level_operator(text) {
            Precedence::Operator
        } else {
            Precedence::Atom
        }
    }

    fn has_top_level_operator(text: &str) -> bool {
        let mut scanner = NestingScanner::default();
        text.chars()
            .any(|character| scanner.is_top_level_operator(character))
    }

    fn needs_temporaries(&self, arguments: &[Code]) -> bool {
        let order: Vec<usize> = self.parsed.placeholders().collect();
        let repeated_or_reordered = order.windows(2).any(|pair| pair[0] >= pair[1]);
        repeated_or_reordered && arguments.iter().any(|argument| !argument.is_plain_name())
    }

    fn inline_with_temporaries(&self, arguments: Vec<Code>) -> Code {
        let mut statements = Vec::new();
        let mut replaced = Vec::new();
        for (index, argument) in arguments.into_iter().enumerate() {
            let temporary = format!("{}{index}", Self::TEMPORARY_PREFIX);
            statements.push(Fragment::Newline);
            statements.push(Fragment::Text(format!("let {temporary} = ")));
            statements.extend(argument.fragments);
            statements.push(Fragment::Text(";".to_string()));
            replaced.push(Code::atom(temporary));
        }
        statements.push(Fragment::Newline);
        statements.extend(self.substitute(&replaced).fragments);
        Code {
            fragments: vec![
                Fragment::Text("{".to_string()),
                Fragment::Indented(statements),
                Fragment::Newline,
                Fragment::Text("}".to_string()),
            ],
            precedence: Precedence::Block,
        }
    }
}

#[derive(Default)]
struct NestingScanner {
    depth: usize,
    in_string: bool,
    escaped: bool,
    previous: Option<char>,
}

impl NestingScanner {
    const QUOTE: char = '"';
    const ESCAPE: char = '\\';
    const RANGE_DOT: char = '.';

    fn is_top_level_operator(&mut self, character: char) -> bool {
        let at_top_level = self.depth == 0 && !self.in_string;
        let closes_range = character == Self::RANGE_DOT && self.previous == Some(Self::RANGE_DOT);
        let separates = character.is_whitespace() || closes_range;
        self.advance(character);
        at_top_level && separates
    }

    fn advance(&mut self, character: char) {
        if character == Self::QUOTE && !self.escaped {
            self.in_string = !self.in_string;
        } else if !self.in_string {
            self.depth = self.depth_after(character);
        }
        self.escaped = character == Self::ESCAPE;
        self.previous = Some(character);
    }

    fn depth_after(&self, character: char) -> usize {
        if Template::OPENERS.contains(&character) {
            self.depth + 1
        } else if Template::CLOSERS.contains(&character) {
            self.depth.saturating_sub(1)
        } else {
            self.depth
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inline(template: &str, arguments: Vec<Code>) -> String {
        Template::parse(template)
            .expect("valid template")
            .inline(arguments)
            .expect("enough arguments")
            .text()
    }

    fn operator(text: &str) -> Code {
        Code::with(text, Precedence::Operator)
    }

    #[test]
    fn substitutes_placeholders_and_braces() {
        assert_eq!(
            inline("println!(\"{{}}\", {0})", vec![Code::atom("x")]),
            "println!(\"{}\", x)"
        );
    }

    #[test]
    fn parenthesizes_operator_arguments_before_a_method() {
        assert_eq!(
            inline("{0}.sqrt()", vec![operator("a - b")]),
            "(a - b).sqrt()"
        );
    }

    #[test]
    fn leaves_isolated_arguments_alone() {
        assert_eq!(
            inline("f({0}, {1})", vec![operator("a + b"), operator("c * d")]),
            "f(a + b, c * d)"
        );
    }

    #[test]
    fn binds_repeated_complex_arguments_to_temporaries() {
        let code = Template::parse("{0} * {0}")
            .unwrap()
            .inline(vec![Code::atom("next()")])
            .unwrap();
        assert_eq!(code.precedence, Precedence::Block);
        assert!(code.text().contains("let template_argument_0 = next();"));
    }

    #[test]
    fn reports_a_malformed_placeholder() {
        assert_eq!(
            Template::parse("{0").err(),
            Some(TemplateError::MalformedPlaceholder)
        );
    }

    #[test]
    fn reports_a_missing_argument() {
        let missing = Template::parse("{0} + {1}")
            .unwrap()
            .inline(vec![Code::atom("x")])
            .map(|code| code.text());
        assert_eq!(missing, Err(TemplateError::MissingArgument(1)));
    }
}
