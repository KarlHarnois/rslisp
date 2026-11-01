pub struct Syntax;

impl Syntax {
    pub const OPEN: char = '(';
    pub const CLOSE: char = ')';
    pub const QUOTE: char = '"';
    pub const COMMENT: char = ';';
    pub const ESCAPE: char = '\\';
    pub const NEWLINE: char = '\n';
    const RESERVED: [char; 4] = ['[', ']', '{', '}'];

    pub fn is_reserved(character: char) -> bool {
        Self::RESERVED.contains(&character)
    }

    pub fn ends_atom(character: char) -> bool {
        character.is_whitespace()
            || [Self::OPEN, Self::CLOSE, Self::QUOTE, Self::COMMENT].contains(&character)
            || Self::is_reserved(character)
    }
}
