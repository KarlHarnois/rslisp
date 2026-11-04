use super::syntax::Syntax;

pub struct Cursor<'a> {
    text: &'a str,
    position: usize,
}

impl<'a> Cursor<'a> {
    pub fn starting_at(text: &'a str, position: usize) -> Self {
        Self { text, position }
    }

    pub fn peek(&self) -> Option<char> {
        self.text[self.position..].chars().next()
    }

    pub fn advance(&mut self) -> Option<char> {
        let next = self.peek()?;
        self.position += next.len_utf8();
        Some(next)
    }

    pub fn advance_while(&mut self, continues: impl Fn(char) -> bool) -> &'a str {
        let start = self.position;
        while self.peek().is_some_and(&continues) {
            self.advance();
        }
        &self.text[start..self.position]
    }

    pub fn skip_whitespace(&mut self) {
        self.advance_while(char::is_whitespace);
    }

    pub fn advance_atom(&mut self) -> &'a str {
        self.advance_while(|character| !Syntax::ends_atom(character))
    }

    pub fn advance_to_line_end(&mut self) -> &'a str {
        self.advance_while(|character| character != Syntax::NEWLINE)
    }

    pub fn position(&self) -> usize {
        self.position
    }
}
