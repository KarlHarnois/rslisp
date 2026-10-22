use super::fragment::{Code, Fragment, Precedence};
use super::operator::{RustOperator, RustText, Side};
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

    pub fn borrows(&self, argument: usize) -> bool {
        self.parsed.borrows(argument)
    }

    pub fn dereferences(&self, argument: usize) -> bool {
        self.parsed.dereferences(argument)
    }

    pub fn inline(&self, arguments: Vec<Code>) -> Result<Code, TemplateError> {
        self.inline_with_places(arguments, &[])
    }

    pub fn inline_with_places(
        &self,
        arguments: Vec<Code>,
        places: &[bool],
    ) -> Result<Code, TemplateError> {
        if let Some(missing) = self
            .parsed
            .placeholders()
            .find(|index| *index >= arguments.len())
        {
            return Err(TemplateError::MissingArgument(missing));
        }
        if let [Piece::Placeholder { index, .. }] = self.parsed.pieces() {
            return Ok(arguments[*index].clone());
        }
        if !places.iter().any(|place| *place) && self.needs_temporaries(&arguments) {
            return Ok(self.inline_with_temporaries(arguments));
        }
        Ok(self.substitute(&arguments, places))
    }

    fn substitute(&self, arguments: &[Code], places: &[bool]) -> Code {
        let pieces = self.pieces_without_deref(places);
        let mut fragments = Vec::new();
        for (position, piece) in pieces.iter().enumerate() {
            match piece {
                Piece::Literal(literal) => fragments.push(Fragment::Text(literal.clone())),
                Piece::Placeholder { index, .. } => fragments.extend(
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

    fn pieces_without_deref(&self, places: &[bool]) -> Vec<Piece> {
        let mut pieces = self.parsed.pieces().to_vec();
        for position in 0..pieces.len() {
            let Piece::Placeholder { index, .. } = &pieces[position] else {
                continue;
            };
            if !places.get(*index).copied().unwrap_or(false) {
                continue;
            }
            if let Some(Piece::Literal(literal)) = position
                .checked_sub(1)
                .and_then(|previous| pieces.get_mut(previous))
                && literal.ends_with('*')
            {
                literal.pop();
            }
        }
        pieces
    }

    fn argument_in_place(&self, argument: &Code, position: usize) -> Code {
        if self.keeps_whole(argument, position) || self.keeps_operator(argument, position) {
            return argument.clone();
        }
        argument.clone().parenthesized()
    }

    fn keeps_whole(&self, argument: &Code, position: usize) -> bool {
        argument.is_atomic() || self.is_isolated(position) || self.holds_prefix(argument, position)
    }

    fn keeps_operator(&self, argument: &Code, position: usize) -> bool {
        let Some((operator, operator_position)) = self.binary_operator() else {
            return false;
        };
        let Some(inner) = RustOperator::outermost(&argument.text()) else {
            return false;
        };
        !inner.needs_parentheses(operator, Self::side(position, operator_position))
    }

    fn side(position: usize, operator_position: usize) -> Side {
        if position < operator_position {
            Side::Left
        } else {
            Side::Right
        }
    }

    fn binary_operator(&self) -> Option<(RustOperator, usize)> {
        let mut found = None;
        for (position, piece) in self.parsed.pieces().iter().enumerate() {
            let Piece::Literal(literal) = piece else {
                continue;
            };
            for token in RustText::new(literal).tokens() {
                let Some(operator) = RustOperator::at(token) else {
                    continue;
                };
                if found.is_some() {
                    return None;
                }
                found = Some((operator, position));
            }
        }
        let (operator, position) = found?;
        self.binary_operands(position)
            .then_some((operator, position))
    }

    fn binary_operands(&self, operator_position: usize) -> bool {
        let has_left = self.parsed.pieces()[..operator_position]
            .iter()
            .any(Self::is_operand);
        let has_right = self.parsed.pieces()[operator_position + 1..]
            .iter()
            .any(Self::is_operand);
        has_left && has_right
    }

    fn is_operand(piece: &Piece) -> bool {
        match piece {
            Piece::Placeholder { .. } => true,
            Piece::Literal(literal) => !literal.trim().is_empty(),
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
            Piece::Placeholder { .. } => None,
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
        RustText::new(text).has_top_level_operator()
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
        statements.extend(self.substitute(&replaced, &[]).fragments);
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
