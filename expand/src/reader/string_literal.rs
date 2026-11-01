use std::ops::Range;

use super::cursor::Cursor;
use super::syntax::Syntax;

pub enum StringProblem {
    UnknownEscape(Range<usize>),
    Unterminated,
}

pub struct StringLiteral {
    pub value: String,
    pub problems: Vec<StringProblem>,
}

pub struct StringLiteralReader<'c, 'a> {
    cursor: &'c mut Cursor<'a>,
    value: String,
    problems: Vec<StringProblem>,
}

impl<'c, 'a> StringLiteralReader<'c, 'a> {
    const SIMPLE_ESCAPES: [(char, char); 7] = [
        ('n', '\n'),
        ('r', '\r'),
        ('t', '\t'),
        ('0', '\0'),
        ('\\', '\\'),
        ('"', '"'),
        ('\'', '\''),
    ];
    const UNICODE_ESCAPE: char = 'u';
    const UNICODE_OPEN: char = '{';
    const UNICODE_CLOSE: char = '}';
    const HEX_RADIX: u32 = 16;

    pub fn read(cursor: &'c mut Cursor<'a>) -> StringLiteral {
        let mut reader = Self {
            cursor,
            value: String::new(),
            problems: Vec::new(),
        };
        reader.cursor.advance();
        reader.read_contents();
        StringLiteral {
            value: reader.value,
            problems: reader.problems,
        }
    }

    fn read_contents(&mut self) {
        loop {
            match self.cursor.advance() {
                None => return self.problems.push(StringProblem::Unterminated),
                Some(Syntax::QUOTE) => return,
                Some(Syntax::ESCAPE) => self.read_escape(),
                Some(character) => self.value.push(character),
            }
        }
    }

    fn read_escape(&mut self) {
        let start = self.cursor.position() - Syntax::ESCAPE.len_utf8();
        match self.escaped_character() {
            Some(character) => self.value.push(character),
            None => self
                .problems
                .push(StringProblem::UnknownEscape(start..self.cursor.position())),
        }
    }

    fn escaped_character(&mut self) -> Option<char> {
        let marker = self.cursor.advance()?;
        if marker == Self::UNICODE_ESCAPE {
            return self.read_unicode_escape();
        }
        Self::SIMPLE_ESCAPES
            .iter()
            .find(|(escape, _)| *escape == marker)
            .map(|(_, character)| *character)
    }

    fn read_unicode_escape(&mut self) -> Option<char> {
        self.expect(Self::UNICODE_OPEN)?;
        let digits = self
            .cursor
            .advance_while(|character| character.is_ascii_hexdigit());
        self.expect(Self::UNICODE_CLOSE)?;
        let code_point = u32::from_str_radix(digits, Self::HEX_RADIX).ok()?;
        char::from_u32(code_point)
    }

    fn expect(&mut self, expected: char) -> Option<()> {
        (self.cursor.peek() == Some(expected)).then(|| {
            self.cursor.advance();
        })
    }
}
