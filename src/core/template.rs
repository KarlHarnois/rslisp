#[derive(Debug, PartialEq, Eq)]
pub struct MalformedPlaceholder;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    Literal(String),
    Placeholder { index: usize, borrowed: bool },
}

#[derive(Clone, Debug)]
pub struct ParsedTemplate {
    pieces: Vec<Piece>,
    text: String,
}

impl ParsedTemplate {
    const BORROW: char = '&';

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
                    let placeholder = Self::placeholder(&mut characters)?;
                    pieces.push(Piece::Literal(std::mem::take(&mut literal)));
                    pieces.push(placeholder);
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

    fn placeholder(
        characters: &mut std::iter::Peekable<std::str::Chars>,
    ) -> Result<Piece, MalformedPlaceholder> {
        let borrowed = characters.next_if_eq(&Self::BORROW).is_some();
        let index = Self::placeholder_index(characters)?;
        Ok(Piece::Placeholder { index, borrowed })
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
            Piece::Placeholder { index, .. } => Some(*index),
            Piece::Literal(_) => None,
        })
    }

    pub fn borrows(&self, argument: usize) -> bool {
        let uses: Vec<bool> = self
            .pieces
            .iter()
            .filter_map(|piece| match piece {
                Piece::Placeholder { index, borrowed } if *index == argument => Some(*borrowed),
                _ => None,
            })
            .collect();
        !uses.is_empty() && uses.iter().all(|borrowed| *borrowed)
    }

    pub fn borrows_any(&self) -> bool {
        self.pieces
            .iter()
            .any(|piece| matches!(piece, Piece::Placeholder { borrowed: true, .. }))
    }

    pub fn indexes(&self, argument: usize) -> bool {
        self.pieces.iter().enumerate().any(|(position, piece)| {
            matches!(piece, Piece::Placeholder { index, .. } if *index == argument)
                && self.pieces.get(position + 1).is_some_and(|next| {
                    matches!(next, Piece::Literal(literal) if literal.trim_start().starts_with('['))
                })
        })
    }

    pub fn dereferences(&self, argument: usize) -> bool {
        let mut found = false;
        for (position, piece) in self.pieces.iter().enumerate() {
            if !matches!(piece, Piece::Placeholder { index, .. } if *index == argument) {
                continue;
            }
            found = true;
            let dereferenced = position
                .checked_sub(1)
                .and_then(|previous| match &self.pieces[previous] {
                    Piece::Literal(literal) => Some(Self::unary_deref(literal)),
                    Piece::Placeholder { .. } => None,
                })
                .unwrap_or(false);
            if !dereferenced {
                return false;
            }
        }
        found
    }

    fn unary_deref(literal: &str) -> bool {
        let Some(remainder) = literal.strip_suffix('*') else {
            return false;
        };
        remainder
            .chars()
            .rev()
            .find(|character| !character.is_whitespace())
            .is_none_or(|character| {
                !character.is_alphanumeric() && character != '_' && !")]}".contains(character)
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
                Piece::Placeholder {
                    index: 0,
                    borrowed: false
                },
                Piece::Literal(">".to_string())
            ]
        );
    }

    #[test]
    fn marks_a_placeholder_that_rust_borrows() {
        let parsed = ParsedTemplate::parse("println!(\"{{}}\", {&0})").expect("a valid template");
        assert!(parsed.borrows(0));
        assert!(parsed.borrows_any());
    }

    #[test]
    fn reports_a_placeholder_before_a_bracket() {
        let parsed = ParsedTemplate::parse("{0} [{1}]").expect("a valid template");
        assert!(parsed.indexes(0));
        assert!(!parsed.indexes(1));
    }

    #[test]
    fn ignores_a_placeholder_before_other_text() {
        let parsed = ParsedTemplate::parse("{0}({1})").expect("a valid template");
        assert!(!parsed.indexes(0));
    }

    #[test]
    fn borrows_only_when_every_use_is_marked() {
        let parsed = ParsedTemplate::parse("{0} + {&0}").expect("a valid template");
        assert!(!parsed.borrows(0));
        assert!(!parsed.borrows(1));
    }

    #[test]
    fn reports_a_borrowed_placeholder_without_a_number() {
        assert_eq!(
            ParsedTemplate::parse("{&x}").err(),
            Some(MalformedPlaceholder)
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
