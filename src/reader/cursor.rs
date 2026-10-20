pub struct Cursor<'a> {
    text: &'a str,
    position: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { text, position: 0 }
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

    pub fn position(&self) -> usize {
        self.position
    }
}
