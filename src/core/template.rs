#[derive(Debug, PartialEq, Eq)]
pub struct MalformedPlaceholder;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    Literal(String),
    Placeholder { index: usize, borrowed: bool },
    TypePlaceholder { name: String },
    BinderPlaceholder { name: String },
}

#[derive(Clone, Debug)]
pub struct ParsedTemplate {
    pieces: Vec<Piece>,
    text: String,
}

impl ParsedTemplate {
    const BORROW: char = '&';
    const BINDER: char = '!';

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
        let name = Self::placeholder_name(characters)?;
        if let Some(binder) = name.strip_prefix(Self::BINDER) {
            return if borrowed || binder.is_empty() {
                Err(MalformedPlaceholder)
            } else {
                Ok(Piece::BinderPlaceholder {
                    name: binder.to_string(),
                })
            };
        }
        match name.parse::<usize>() {
            Ok(index) => Ok(Piece::Placeholder { index, borrowed }),
            Err(_) if borrowed => Err(MalformedPlaceholder),
            Err(_) => Ok(Piece::TypePlaceholder { name }),
        }
    }

    fn placeholder_name(
        characters: &mut impl Iterator<Item = char>,
    ) -> Result<String, MalformedPlaceholder> {
        let mut name = String::new();
        for next in characters {
            if next == '}' {
                return if name.is_empty() {
                    Err(MalformedPlaceholder)
                } else {
                    Ok(name)
                };
            }
            name.push(next);
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
            Piece::Literal(_) | Piece::TypePlaceholder { .. } | Piece::BinderPlaceholder { .. } => {
                None
            }
        })
    }

    pub fn type_placeholders(&self) -> impl Iterator<Item = &str> + '_ {
        self.pieces.iter().filter_map(|piece| match piece {
            Piece::TypePlaceholder { name } => Some(name.as_str()),
            Piece::Literal(_) | Piece::Placeholder { .. } | Piece::BinderPlaceholder { .. } => None,
        })
    }

    pub fn binder_names(&self) -> Vec<&str> {
        let mut names = Vec::new();
        for piece in &self.pieces {
            if let Piece::BinderPlaceholder { name } = piece
                && !names.contains(&name.as_str())
            {
                names.push(name.as_str());
            }
        }
        names
    }

    pub fn with_binders(&self, renames: &[(String, String)]) -> Self {
        let pieces = self
            .pieces
            .iter()
            .map(|piece| match piece {
                Piece::BinderPlaceholder { name } => {
                    let fresh = renames
                        .iter()
                        .find(|(binder, _)| binder == name)
                        .map_or(name.as_str(), |(_, fresh)| fresh.as_str());
                    Piece::Literal(fresh.to_string())
                }
                other => other.clone(),
            })
            .collect();
        Self::from_pieces(pieces)
    }

    fn from_pieces(pieces: Vec<Piece>) -> Self {
        let mut merged: Vec<Piece> = Vec::new();
        for piece in pieces {
            if let (Piece::Literal(text), Some(Piece::Literal(previous))) =
                (&piece, merged.last_mut())
            {
                previous.push_str(text);
            } else {
                merged.push(piece);
            }
        }
        let text = merged.iter().map(Self::rendered_piece).collect();
        Self {
            pieces: merged,
            text,
        }
    }

    fn rendered_piece(piece: &Piece) -> String {
        match piece {
            Piece::Literal(literal) => literal.replace('{', "{{").replace('}', "}}"),
            Piece::Placeholder { index, borrowed } => {
                let borrow = if *borrowed {
                    Self::BORROW.to_string()
                } else {
                    String::new()
                };
                format!("{{{borrow}{index}}}")
            }
            Piece::TypePlaceholder { name } => format!("{{{name}}}"),
            Piece::BinderPlaceholder { name } => format!("{{!{name}}}"),
        }
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

    pub fn called_arguments(&self, argument: usize) -> Option<Vec<&str>> {
        let literal = self.call_literal(argument)?;
        Self::arguments_after_open_parenthesis(literal)
    }

    fn call_literal(&self, argument: usize) -> Option<&str> {
        let mut uses = self
            .pieces
            .iter()
            .enumerate()
            .filter_map(|(position, piece)| {
                matches!(piece, Piece::Placeholder { index, .. } if *index == argument)
                    .then_some(position)
            });
        let position = uses.next()?;
        if uses.next().is_some() {
            return None;
        }
        match self.pieces.get(position + 1)? {
            Piece::Literal(literal) => Some(literal),
            Piece::Placeholder { .. }
            | Piece::TypePlaceholder { .. }
            | Piece::BinderPlaceholder { .. } => None,
        }
    }

    fn arguments_after_open_parenthesis(literal: &str) -> Option<Vec<&str>> {
        let inner = literal.trim_start().strip_prefix('(')?;
        let close = inner.find(')')?;
        let arguments: Vec<&str> = inner[..close].split(',').map(str::trim).collect();
        (!arguments.iter().any(|name| name.is_empty())).then_some(arguments)
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
                    Piece::Placeholder { .. }
                    | Piece::TypePlaceholder { .. }
                    | Piece::BinderPlaceholder { .. } => None,
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
    fn reports_an_empty_placeholder() {
        assert_eq!(
            ParsedTemplate::parse("{}").err(),
            Some(MalformedPlaceholder)
        );
    }

    #[test]
    fn reads_the_arguments_of_a_called_placeholder() {
        let parsed = ParsedTemplate::parse("{0}.sort_by(|left, right| {1}(left, right).cmp(&0))")
            .expect("a valid template");
        assert_eq!(parsed.called_arguments(1), Some(vec!["left", "right"]));
    }

    #[test]
    fn rejects_a_placeholder_that_is_not_called() {
        let parsed = ParsedTemplate::parse("{1}.len()").expect("a valid template");
        assert_eq!(parsed.called_arguments(1), None);
    }

    #[test]
    fn rejects_a_repeated_called_placeholder() {
        let parsed = ParsedTemplate::parse("{1}(a) + {1}(b)").expect("a valid template");
        assert_eq!(parsed.called_arguments(1), None);
    }

    #[test]
    fn rejects_a_call_with_no_arguments() {
        let parsed = ParsedTemplate::parse("{1}()").expect("a valid template");
        assert_eq!(parsed.called_arguments(1), None);
    }

    #[test]
    fn rejects_a_placeholder_followed_by_another_placeholder() {
        let parsed = ParsedTemplate::parse("{1}{0}").expect("a valid template");
        assert_eq!(parsed.called_arguments(1), None);
    }

    #[test]
    fn rejects_a_trailing_placeholder() {
        let parsed = ParsedTemplate::parse("{1}").expect("a valid template");
        assert_eq!(parsed.called_arguments(1), None);
    }

    #[test]
    fn rejects_an_uncalled_argument_zero() {
        let parsed = ParsedTemplate::parse("{0}.sort_by(|left, right| {1}(left, right))")
            .expect("a valid template");
        assert_eq!(parsed.called_arguments(0), None);
    }

    #[test]
    fn names_a_type_placeholder() {
        let parsed = ParsedTemplate::parse("{target}").expect("a named placeholder");
        assert_eq!(parsed.type_placeholders().collect::<Vec<_>>(), ["target"]);
        assert_eq!(
            parsed.placeholders().collect::<Vec<_>>(),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn reports_a_borrowed_type_placeholder() {
        assert_eq!(
            ParsedTemplate::parse("{&target}").err(),
            Some(MalformedPlaceholder)
        );
    }

    #[test]
    fn names_each_binder_once() {
        let parsed =
            ParsedTemplate::parse("|{!left}, {!right}| {!left}").expect("a template with binders");
        assert_eq!(parsed.binder_names(), ["left", "right"]);
        assert_eq!(
            parsed.placeholders().collect::<Vec<_>>(),
            Vec::<usize>::new()
        );
        assert_eq!(
            parsed.type_placeholders().collect::<Vec<_>>(),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn reports_a_borrowed_binder_placeholder() {
        assert_eq!(
            ParsedTemplate::parse("{&!left}").err(),
            Some(MalformedPlaceholder)
        );
    }

    #[test]
    fn reports_an_empty_binder_placeholder() {
        assert_eq!(
            ParsedTemplate::parse("{!}").err(),
            Some(MalformedPlaceholder)
        );
    }
}
