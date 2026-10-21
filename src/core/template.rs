#[derive(Debug, PartialEq, Eq)]
pub struct MalformedPlaceholder;

#[derive(Debug, PartialEq, Eq)]
pub enum Piece {
    Literal(String),
    Placeholder(usize),
}

pub struct ParsedTemplate {
    pieces: Vec<Piece>,
    text: String,
}

impl ParsedTemplate {
    pub fn parse(text: &str) -> Result<Self, MalformedPlaceholder> {
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
    ) -> Result<usize, MalformedPlaceholder> {
        let mut digits = String::new();
        for next in characters {
            if next == '}' {
                return digits.parse().map_err(|_| MalformedPlaceholder);
            }
            digits.push(next);
        }
        Err(MalformedPlaceholder)
    }

    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn placeholders(&self) -> impl Iterator<Item = usize> + '_ {
        self.pieces.iter().filter_map(|piece| match piece {
            Piece::Placeholder(index) => Some(*index),
            Piece::Literal(_) => None,
        })
    }

    pub fn is_path(&self) -> bool {
        match self.pieces.as_slice() {
            [Piece::Literal(literal)] => literal
                .chars()
                .all(|character| character.is_alphanumeric() || "_:".contains(character)),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MalformedPlaceholder, ParsedTemplate, Piece};

    #[test]
    fn splits_literals_from_placeholders() {
        let parsed = ParsedTemplate::parse("Vec<{0}>").expect("a valid template");
        assert_eq!(
            parsed.pieces(),
            [
                Piece::Literal("Vec<".to_string()),
                Piece::Placeholder(0),
                Piece::Literal(">".to_string())
            ]
        );
    }

    #[test]
    fn reports_an_unclosed_placeholder() {
        assert_eq!(
            ParsedTemplate::parse("{0").err(),
            Some(MalformedPlaceholder)
        );
    }

    #[test]
    fn reports_a_placeholder_without_a_number() {
        assert_eq!(
            ParsedTemplate::parse("{name}").err(),
            Some(MalformedPlaceholder)
        );
    }
}
