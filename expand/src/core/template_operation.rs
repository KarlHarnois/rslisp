use super::kernel::PrimitiveType;
use super::template::Piece;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operand {
    Argument(usize),
    Constant(Option<i128>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Division {
    pub dividend: Option<Operand>,
    pub divisor: Operand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lexeme<'t> {
    Argument(usize),
    Number(&'t str),
    Symbol(char),
    Other,
}

pub struct TemplateOperation<'t> {
    lexemes: Vec<Lexeme<'t>>,
}

impl<'t> TemplateOperation<'t> {
    const TUPLE_SYMBOLS: &'static str = "(),";
    const OPERATOR_SYMBOLS: &'static str = "+-*/%!<>=&|^(),";
    const DIVISION_SYMBOLS: &'static str = "/%";
    const NEGATION: char = '-';
    const NUMBER_SEPARATORS: &'static str = "_.";
    const DECIMAL_POINT: char = '.';
    const RANGE: &'static str = "..";
    const DIGIT_SEPARATOR: &'static str = "_";

    pub fn of(pieces: &'t [Piece]) -> Self {
        Self {
            lexemes: pieces.iter().flat_map(Self::lexemes_of).collect(),
        }
    }

    pub fn is_tuple(&self) -> bool {
        self.lexemes.iter().all(|lexeme| match lexeme {
            Lexeme::Argument(_) | Lexeme::Number(_) => true,
            Lexeme::Symbol(symbol) => Self::TUPLE_SYMBOLS.contains(*symbol),
            Lexeme::Other => false,
        })
    }

    pub fn is_operator(&self) -> bool {
        !self.is_tuple() && !self.lexemes.contains(&Lexeme::Other)
    }

    pub fn is_negation(&self) -> bool {
        matches!(
            self.lexemes.as_slice(),
            [Lexeme::Symbol(Self::NEGATION), Lexeme::Argument(_)]
        )
    }

    pub fn divisions(&self) -> Vec<Division> {
        self.lexemes
            .iter()
            .enumerate()
            .filter(|(_, lexeme)| Self::divides(lexeme))
            .filter_map(|(position, _)| self.division_at(position))
            .collect()
    }

    fn lexemes_of(piece: &'t Piece) -> Vec<Lexeme<'t>> {
        match piece {
            Piece::Literal(literal) => Self::lex(literal),
            Piece::Placeholder { index, .. } => vec![Lexeme::Argument(*index)],
            Piece::TypePlaceholder { .. } | Piece::BinderPlaceholder { .. } => vec![Lexeme::Other],
        }
    }

    fn lex(literal: &'t str) -> Vec<Lexeme<'t>> {
        let mut lexemes = Vec::new();
        let mut rest = literal.trim_start();
        while let Some(first) = rest.chars().next() {
            let length = Self::lexeme_length(rest, first);
            lexemes.push(Self::lexeme(&rest[..length], first));
            rest = rest[length..].trim_start();
        }
        lexemes
    }

    fn lexeme_length(rest: &str, first: char) -> usize {
        if !first.is_ascii_digit() {
            return first.len_utf8();
        }
        rest.find(|character: char| {
            !character.is_alphanumeric() && !Self::NUMBER_SEPARATORS.contains(character)
        })
        .unwrap_or(rest.len())
    }

    fn lexeme(text: &'t str, first: char) -> Lexeme<'t> {
        match first {
            digit if digit.is_ascii_digit() && Self::is_number_literal(text) => {
                Lexeme::Number(text)
            }
            symbol if Self::OPERATOR_SYMBOLS.contains(symbol) => Lexeme::Symbol(symbol),
            _ => Lexeme::Other,
        }
    }

    fn is_number_literal(text: &str) -> bool {
        let fractions_start_with_digits = text.split(Self::DECIMAL_POINT).skip(1).all(|fraction| {
            fraction
                .chars()
                .next()
                .is_none_or(|digit| digit.is_ascii_digit())
        });
        fractions_start_with_digits && !text.contains(Self::RANGE)
    }

    fn divides(lexeme: &Lexeme) -> bool {
        matches!(lexeme, Lexeme::Symbol(symbol) if Self::DIVISION_SYMBOLS.contains(*symbol))
    }

    fn division_at(&self, position: usize) -> Option<Division> {
        let divisor = self.operand_after(position + 1)?;
        let dividend = position
            .checked_sub(1)
            .and_then(|before| self.operand(before));
        Some(Division { dividend, divisor })
    }

    fn operand_after(&self, position: usize) -> Option<Operand> {
        match self.lexemes.get(position)? {
            Lexeme::Symbol(Self::NEGATION) => self.negated_operand(position + 1),
            _ => self.operand(position),
        }
    }

    fn negated_operand(&self, position: usize) -> Option<Operand> {
        match self.lexemes.get(position)? {
            Lexeme::Number(text) => Some(Operand::Constant(
                Self::integer_value(text).map(|value| -value),
            )),
            _ => None,
        }
    }

    fn operand(&self, position: usize) -> Option<Operand> {
        match self.lexemes.get(position)? {
            Lexeme::Argument(index) => Some(Operand::Argument(*index)),
            Lexeme::Number(text) => Some(Operand::Constant(Self::integer_value(text))),
            Lexeme::Symbol(_) | Lexeme::Other => None,
        }
    }

    fn integer_value(text: &str) -> Option<i128> {
        let digits = PrimitiveType::all()
            .filter(|primitive| primitive.is_integer())
            .find_map(|primitive| text.strip_suffix(primitive.name()))
            .unwrap_or(text);
        digits.replace(Self::DIGIT_SEPARATOR, "").parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::{Division, Operand};
    use crate::core::ParsedTemplate;

    struct OperationFixture;

    impl OperationFixture {
        fn parsed(text: &str) -> ParsedTemplate {
            ParsedTemplate::parse(text).expect("a valid template")
        }

        fn divisions(text: &str) -> Vec<Division> {
            Self::parsed(text).operation().divisions()
        }
    }

    #[test]
    fn names_an_operator_over_placeholders() {
        for text in [
            "{0} + {1}",
            "-{0}",
            "!{0}",
            "{&0} <= {&1}",
            "{0} + 1",
            "{0} * 2_i64",
            "{0} * 1.5",
        ] {
            let parsed = OperationFixture::parsed(text);
            assert!(parsed.operation().is_operator(), "{text}");
            assert!(!parsed.operation().is_tuple(), "{text}");
        }
    }

    #[test]
    fn names_a_tuple_of_placeholders_and_numbers() {
        for text in ["({0}, {1})", "({0}, 1)", "()"] {
            let parsed = OperationFixture::parsed(text);
            assert!(parsed.operation().is_tuple(), "{text}");
            assert!(!parsed.operation().is_operator(), "{text}");
        }
    }

    #[test]
    fn rejects_calls_ranges_and_constants_as_operations() {
        for text in [
            "{0}.len()",
            "vec![{0}]",
            "&{0}[{1}]",
            "Box::new({0})",
            "{0}.parse::<{target}>()",
            "None",
            "|{!item}| {0}",
            "{0} + 1.max(2)",
            "1..{0}",
        ] {
            let parsed = OperationFixture::parsed(text);
            assert!(!parsed.operation().is_operator(), "{text}");
            assert!(!parsed.operation().is_tuple(), "{text}");
        }
    }

    #[test]
    fn names_a_negation() {
        assert!(OperationFixture::parsed("-{0}").operation().is_negation());
        assert!(
            !OperationFixture::parsed("{0} - {1}")
                .operation()
                .is_negation()
        );
        assert!(!OperationFixture::parsed("!{0}").operation().is_negation());
    }

    #[test]
    fn finds_a_division_between_placeholders() {
        assert_eq!(
            OperationFixture::divisions("{1} % {0}"),
            [Division {
                dividend: Some(Operand::Argument(1)),
                divisor: Operand::Argument(0),
            }]
        );
        assert!(OperationFixture::divisions("{0} + {1}").is_empty());
    }

    #[test]
    fn reads_a_number_divisor() {
        for (text, value) in [
            ("{0} / 0", Some(0)),
            ("{0} % -1", Some(-1)),
            ("{0} / 1_000_i64", Some(1000)),
            ("{0} / 2.5", None),
        ] {
            assert_eq!(
                OperationFixture::divisions(text),
                [Division {
                    dividend: Some(Operand::Argument(0)),
                    divisor: Operand::Constant(value),
                }],
                "{text}"
            );
        }
    }

    #[test]
    fn reads_a_number_dividend() {
        assert_eq!(
            OperationFixture::divisions("10 / {0}"),
            [Division {
                dividend: Some(Operand::Constant(Some(10))),
                divisor: Operand::Argument(0),
            }]
        );
        assert_eq!(
            OperationFixture::divisions("({0}) / {1}"),
            [Division {
                dividend: None,
                divisor: Operand::Argument(1),
            }]
        );
    }

    #[test]
    fn skips_a_division_by_a_negated_placeholder() {
        assert!(OperationFixture::divisions("{0} / -{1}").is_empty());
        assert!(OperationFixture::divisions("{0} / ({1})").is_empty());
    }
}
