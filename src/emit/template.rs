use super::fragment::{Code, Fragment, Precedence};

#[derive(Debug, PartialEq, Eq)]
pub enum TemplateError {
    MalformedPlaceholder,
    MissingArgument(usize),
}

#[derive(Debug, PartialEq, Eq)]
enum Piece {
    Literal(String),
    Placeholder(usize),
}

pub struct Template {
    pieces: Vec<Piece>,
    text: String,
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
        let mut pieces = Vec::new();
        let mut literal = String::new();
        let mut characters = text.chars().peekable();
        while let Some(character) = characters.next() {
            match (character, characters.peek()) {
                ('{', Some('{')) | ('}', Some('}')) => {
                    characters.next();
                    literal.push(character);
                }
                ('{', _) => {
                    let index = Self::placeholder_index(&mut characters)?;
                    pieces.push(Piece::Literal(std::mem::take(&mut literal)));
                    pieces.push(Piece::Placeholder(index));
                }
                _ => literal.push(character),
            }
        }
        pieces.push(Piece::Literal(literal));
        pieces.retain(|piece| *piece != Piece::Literal(String::new()));
        Ok(Self {
            pieces,
            text: text.to_string(),
        })
    }

    fn placeholder_index(
        characters: &mut impl Iterator<Item = char>,
    ) -> Result<usize, TemplateError> {
        let mut digits = String::new();
        for next in characters {
            if next == '}' {
                return digits
                    .parse()
                    .map_err(|_| TemplateError::MalformedPlaceholder);
            }
            digits.push(next);
        }
        Err(TemplateError::MalformedPlaceholder)
    }

    pub fn is_path(&self) -> bool {
        match self.pieces.as_slice() {
            [Piece::Literal(literal)] => literal
                .chars()
                .all(|character| character.is_alphanumeric() || "_:".contains(character)),
            _ => false,
        }
    }

    pub fn inline(&self, arguments: Vec<Code>) -> Result<Code, TemplateError> {
        if let Some(missing) = self.placeholders().find(|index| *index >= arguments.len()) {
            return Err(TemplateError::MissingArgument(missing));
        }
        if let [Piece::Placeholder(index)] = self.pieces.as_slice() {
            return Ok(arguments[*index].clone());
        }
        if self.needs_temporaries(&arguments) {
            return Ok(self.inline_with_temporaries(arguments));
        }
        Ok(self.substitute(&arguments))
    }

    fn substitute(&self, arguments: &[Code]) -> Code {
        let mut fragments = Vec::new();
        for (position, piece) in self.pieces.iter().enumerate() {
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
        match self.pieces.get(position)? {
            Piece::Literal(literal) => Some(literal),
            Piece::Placeholder(_) => None,
        }
    }

    fn precedence(&self) -> Precedence {
        let text = self.text.trim();
        if text.starts_with(Self::UNARY_OPERATORS) {
            Precedence::Prefix
        } else if Self::has_top_level_space(text) {
            Precedence::Operator
        } else {
            Precedence::Atom
        }
    }

    fn has_top_level_space(text: &str) -> bool {
        let mut scanner = NestingScanner::default();
        text.chars()
            .any(|character| scanner.is_top_level_space(character))
    }

    fn placeholders(&self) -> impl Iterator<Item = usize> + '_ {
        self.pieces.iter().filter_map(|piece| match piece {
            Piece::Placeholder(index) => Some(*index),
            Piece::Literal(_) => None,
        })
    }

    fn needs_temporaries(&self, arguments: &[Code]) -> bool {
        let order: Vec<usize> = self.placeholders().collect();
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
}

impl NestingScanner {
    const QUOTE: char = '"';
    const ESCAPE: char = '\\';

    fn is_top_level_space(&mut self, character: char) -> bool {
        let found = character.is_whitespace() && self.depth == 0 && !self.in_string;
        self.advance(character);
        found
    }

    fn advance(&mut self, character: char) {
        if character == Self::QUOTE && !self.escaped {
            self.in_string = !self.in_string;
        } else if !self.in_string {
            self.depth = self.depth_after(character);
        }
        self.escaped = character == Self::ESCAPE;
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
    fn reports_an_unclosed_placeholder() {
        assert_eq!(
            Template::parse("{0").err(),
            Some(TemplateError::MalformedPlaceholder)
        );
    }

    #[test]
    fn reports_a_placeholder_without_a_number() {
        assert_eq!(
            Template::parse("{name}").err(),
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
