#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Associativity {
    Left,
    Associative,
    NonAssociative,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RustOperator {
    symbol: &'static str,
    precedence: u8,
    associativity: Associativity,
}

impl RustOperator {
    const TABLE: [(&'static str, u8, Associativity); 19] = [
        ("=", 0, Associativity::NonAssociative),
        ("||", 1, Associativity::Associative),
        ("&&", 2, Associativity::Associative),
        ("==", 3, Associativity::NonAssociative),
        ("!=", 3, Associativity::NonAssociative),
        ("<", 3, Associativity::NonAssociative),
        (">", 3, Associativity::NonAssociative),
        ("<=", 3, Associativity::NonAssociative),
        (">=", 3, Associativity::NonAssociative),
        ("|", 4, Associativity::Left),
        ("^", 5, Associativity::Left),
        ("&", 6, Associativity::Left),
        ("<<", 7, Associativity::Left),
        (">>", 7, Associativity::Left),
        ("+", 8, Associativity::Left),
        ("-", 8, Associativity::Left),
        ("*", 9, Associativity::Left),
        ("/", 9, Associativity::Left),
        ("%", 9, Associativity::Left),
    ];

    pub fn and() -> Self {
        Self::at("&&").expect("the table holds and")
    }

    pub fn or() -> Self {
        Self::at("||").expect("the table holds or")
    }

    pub fn symbol(self) -> &'static str {
        self.symbol
    }

    pub fn at(token: &str) -> Option<Self> {
        Self::TABLE
            .iter()
            .find(|(symbol, ..)| *symbol == token)
            .map(|(symbol, precedence, associativity)| Self {
                symbol,
                precedence: *precedence,
                associativity: *associativity,
            })
    }

    pub fn outermost(text: &str) -> Option<Self> {
        RustText::new(text)
            .tokens()
            .into_iter()
            .filter_map(Self::at)
            .min_by_key(|operator| operator.precedence)
    }

    pub fn needs_parentheses(self, outer: Self, side: Side) -> bool {
        if self.precedence > outer.precedence {
            return false;
        }
        if self.precedence < outer.precedence {
            return true;
        }
        match outer.associativity {
            Associativity::Associative => false,
            Associativity::Left => side == Side::Right,
            Associativity::NonAssociative => true,
        }
    }
}

pub struct RustText<'a> {
    text: &'a str,
}

impl<'a> RustText<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { text }
    }

    pub fn has_top_level_operator(&self) -> bool {
        self.tokens().len() > 1
    }

    pub fn tokens(&self) -> Vec<&'a str> {
        TokenScanner::scan(self.text).tokens
    }

    pub fn closes_every_delimiter(&self) -> bool {
        TokenScanner::scan(self.text).depth == 0
    }
}

struct TokenScanner<'a> {
    text: &'a str,
    tokens: Vec<&'a str>,
    start: Option<usize>,
    depth: usize,
    literal: Literal,
    escaped: bool,
}

impl<'a> TokenScanner<'a> {
    fn scan(text: &'a str) -> Self {
        let mut scanner = Self {
            text,
            tokens: Vec::new(),
            start: None,
            depth: 0,
            literal: Literal::Outside,
            escaped: false,
        };
        let mut characters = text.char_indices().peekable();
        while let Some((position, character)) = characters.next() {
            if scanner.consume_literal(character) {
                continue;
            }
            if scanner.consume_range(position, character, &mut characters) {
                continue;
            }
            if scanner.consume_delimiter(position, character) {
                continue;
            }
            if character.is_whitespace() && scanner.depth == 0 {
                scanner.flush(position);
            } else {
                scanner.start.get_or_insert(position);
            }
        }
        scanner.flush(text.len());
        scanner
    }

    fn consume_literal(&mut self, character: char) -> bool {
        if self.literal == Literal::Outside {
            return false;
        }
        if self.escaped {
            self.escaped = false;
        } else if character == '\\' {
            self.escaped = true;
        } else if self.literal.closes(character) {
            self.literal = Literal::Outside;
        }
        true
    }

    fn consume_range(
        &mut self,
        position: usize,
        character: char,
        characters: &mut std::iter::Peekable<std::str::CharIndices<'a>>,
    ) -> bool {
        if character != '.' || self.depth != 0 {
            return false;
        }
        if characters.peek().is_none_or(|(_, next)| *next != '.') {
            return false;
        }
        self.flush(position);
        characters.next();
        self.tokens.push("..");
        true
    }

    fn consume_delimiter(&mut self, position: usize, character: char) -> bool {
        match character {
            '"' => self.literal = Literal::String,
            '\'' => self.literal = Literal::Character,
            '(' | '[' | '{' => self.depth += 1,
            ')' | ']' | '}' => self.depth = self.depth.saturating_sub(1),
            _ => return false,
        }
        self.start.get_or_insert(position);
        true
    }

    fn flush(&mut self, position: usize) {
        if let Some(start) = self.start.take() {
            self.tokens.push(&self.text[start..position]);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Literal {
    Outside,
    String,
    Character,
}

impl Literal {
    fn closes(self, character: char) -> bool {
        match self {
            Literal::Outside => false,
            Literal::String => character == '"',
            Literal::Character => character == '\'',
        }
    }
}
