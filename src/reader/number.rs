use super::form::{Number, NumberValue};

pub enum NumberError {
    Malformed,
    TooLarge,
}

pub struct NumberLiteral<'a> {
    text: &'a str,
}

impl<'a> NumberLiteral<'a> {
    const SIGNS: [char; 2] = ['+', '-'];
    const DIGIT_SEPARATOR: char = '_';
    const DECIMAL_POINT: char = '.';
    const EXPONENT_MARKERS: [char; 2] = ['e', 'E'];

    pub fn new(text: &'a str) -> Self {
        Self { text }
    }

    pub fn is_number_shaped(&self) -> bool {
        self.unsigned()
            .starts_with(|character: char| character.is_ascii_digit())
    }

    pub fn parse(&self) -> Result<Number, NumberError> {
        let value = if Self::is_digit_run(self.unsigned()) {
            NumberValue::Integer(self.parse_integer()?)
        } else if Self::is_float(self.unsigned()) {
            NumberValue::Float(self.parse_float())
        } else {
            return Err(NumberError::Malformed);
        };
        Ok(Number::new(self.text, value))
    }

    fn unsigned(&self) -> &'a str {
        self.text.strip_prefix(Self::SIGNS).unwrap_or(self.text)
    }

    fn parse_integer(&self) -> Result<i128, NumberError> {
        self.cleaned().parse().map_err(|_| NumberError::TooLarge)
    }

    fn parse_float(&self) -> f64 {
        self.cleaned()
            .parse()
            .expect("validated float literal parses")
    }

    fn cleaned(&self) -> String {
        let without_plus = self.text.strip_prefix('+').unwrap_or(self.text);
        without_plus.replace(Self::DIGIT_SEPARATOR, "")
    }

    fn is_float(text: &str) -> bool {
        match text.split_once(Self::EXPONENT_MARKERS) {
            Some((mantissa, exponent)) => {
                Self::is_mantissa(mantissa) && Self::is_exponent(exponent)
            }
            None => Self::is_fraction(text),
        }
    }

    fn is_mantissa(text: &str) -> bool {
        Self::is_digit_run(text) || Self::is_fraction(text)
    }

    fn is_fraction(text: &str) -> bool {
        text.split_once(Self::DECIMAL_POINT)
            .is_some_and(|(whole, fraction)| {
                Self::is_digit_run(whole) && Self::is_digit_run(fraction)
            })
    }

    fn is_exponent(text: &str) -> bool {
        Self::is_digit_run(text.strip_prefix(Self::SIGNS).unwrap_or(text))
    }

    fn is_digit_run(text: &str) -> bool {
        text.starts_with(|character: char| character.is_ascii_digit())
            && text
                .chars()
                .all(|character| character.is_ascii_digit() || character == Self::DIGIT_SEPARATOR)
    }
}
