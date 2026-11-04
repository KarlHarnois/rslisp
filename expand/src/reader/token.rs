use std::ops::Range;

use super::cursor::Cursor;
use super::number::NumberLiteral;
use super::string_literal::StringLiteralReader;
use super::syntax::Syntax;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Open,
    Close,
    String,
    Number,
    Symbol,
    Comment,
    Reserved,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub range: Range<usize>,
}

pub struct TokenScan<'a> {
    cursor: Cursor<'a>,
}

impl<'a> TokenScan<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            cursor: Cursor::starting_at(text, 0),
        }
    }
}

impl Iterator for TokenScan<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        self.cursor.skip_whitespace();
        let start = self.cursor.position();
        let kind = self.scan_kind(self.cursor.peek()?);
        Some(Token {
            kind,
            range: start..self.cursor.position(),
        })
    }
}

impl TokenScan<'_> {
    fn scan_kind(&mut self, next: char) -> TokenKind {
        match next {
            Syntax::OPEN => self.skip_character(TokenKind::Open),
            Syntax::CLOSE => self.skip_character(TokenKind::Close),
            Syntax::QUOTE => self.scan_string(),
            Syntax::COMMENT => self.scan_comment(),
            reserved if Syntax::is_reserved(reserved) => self.skip_character(TokenKind::Reserved),
            _ => self.scan_atom(),
        }
    }

    fn skip_character(&mut self, kind: TokenKind) -> TokenKind {
        self.cursor.advance();
        kind
    }

    fn scan_string(&mut self) -> TokenKind {
        StringLiteralReader::read(&mut self.cursor);
        TokenKind::String
    }

    fn scan_comment(&mut self) -> TokenKind {
        self.cursor.advance_to_line_end();
        TokenKind::Comment
    }

    fn scan_atom(&mut self) -> TokenKind {
        if NumberLiteral::new(self.cursor.advance_atom()).is_number_shaped() {
            TokenKind::Number
        } else {
            TokenKind::Symbol
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TokenKind, TokenScan};

    struct ScanFixture;

    impl ScanFixture {
        fn tokens(text: &str) -> Vec<(TokenKind, &str)> {
            TokenScan::new(text)
                .map(|token| (token.kind, &text[token.range]))
                .collect()
        }
    }

    #[test]
    fn a_partial_line_scans_up_to_its_end() {
        assert_eq!(
            ScanFixture::tokens("(defn f \"abc"),
            [
                (TokenKind::Open, "("),
                (TokenKind::Symbol, "defn"),
                (TokenKind::Symbol, "f"),
                (TokenKind::String, "\"abc"),
            ]
        );
    }

    #[test]
    fn a_comment_runs_to_the_end_of_its_line() {
        assert_eq!(
            ScanFixture::tokens("1 ; (two \"x\n2"),
            [
                (TokenKind::Number, "1"),
                (TokenKind::Comment, "; (two \"x"),
                (TokenKind::Number, "2"),
            ]
        );
    }

    #[test]
    fn a_string_keeps_its_escaped_quote_and_ends_at_its_closing_quote() {
        assert_eq!(
            ScanFixture::tokens("\"a\\\"b\")"),
            [(TokenKind::String, "\"a\\\"b\""), (TokenKind::Close, ")")]
        );
    }

    #[test]
    fn an_atom_ends_where_the_reader_ends_it() {
        assert_eq!(
            ScanFixture::tokens(":x-1;c\n-2.5[é]"),
            [
                (TokenKind::Symbol, ":x-1"),
                (TokenKind::Comment, ";c"),
                (TokenKind::Number, "-2.5"),
                (TokenKind::Reserved, "["),
                (TokenKind::Symbol, "é"),
                (TokenKind::Reserved, "]"),
            ]
        );
    }

    #[test]
    fn whitespace_alone_holds_no_token() {
        assert_eq!(ScanFixture::tokens(" \n\t"), []);
    }
}
