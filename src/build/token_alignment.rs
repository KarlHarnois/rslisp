use crate::emit::SpanMap;

pub(crate) struct TokenAlignment {
    original: Vec<LocatedToken>,
    formatted: Vec<LocatedToken>,
}

struct LocatedToken {
    text: String,
    start: usize,
    end: usize,
}

impl TokenAlignment {
    pub(crate) fn new(original: &str, formatted: &str) -> Self {
        Self {
            original: RustTokens::scan(original),
            formatted: RustTokens::scan(formatted),
        }
    }

    pub(crate) fn remap(&self, span_map: &SpanMap) -> Option<SpanMap> {
        Some(span_map.moved(&self.boundaries()?))
    }

    fn boundaries(&self) -> Option<Vec<usize>> {
        let pairs = self.align()?;
        if pairs.is_empty() {
            return None;
        }
        Some(Self::map(
            &pairs,
            self.original.last()?.end,
            self.formatted.last()?.end,
        ))
    }

    fn align(&self) -> Option<Vec<(usize, usize, usize, usize)>> {
        let original = self.original.len();
        let formatted = self.formatted.len();
        let mut lengths = vec![vec![0usize; formatted + 1]; original + 1];
        for left in (0..original).rev() {
            for right in (0..formatted).rev() {
                lengths[left][right] = if self.original[left].text == self.formatted[right].text {
                    lengths[left + 1][right + 1] + 1
                } else {
                    lengths[left + 1][right].max(lengths[left][right + 1])
                };
            }
        }
        let mut pairs = Vec::new();
        let (mut left, mut right) = (0, 0);
        while left < original && right < formatted {
            if self.original[left].text == self.formatted[right].text {
                pairs.push((
                    self.original[left].start,
                    self.original[left].end,
                    self.formatted[right].start,
                    self.formatted[right].end,
                ));
                left += 1;
                right += 1;
            } else if lengths[left + 1][right] >= lengths[left][right + 1] {
                left += 1;
            } else {
                right += 1;
            }
        }
        Some(pairs)
    }

    fn map(
        pairs: &[(usize, usize, usize, usize)],
        original_end: usize,
        formatted_end: usize,
    ) -> Vec<usize> {
        let mut boundaries = vec![0; original_end + 1];
        let mut original_cursor = 0;
        let mut formatted_cursor = 0;
        for (original_start, original_token_end, formatted_start, formatted_token_end) in
            pairs.iter().copied()
        {
            Self::fill_gap(
                &mut boundaries,
                original_cursor..original_start,
                formatted_cursor..formatted_start,
            );
            let length = original_token_end - original_start;
            for offset in 0..=length {
                boundaries[original_start + offset] =
                    formatted_start + offset * (formatted_token_end - formatted_start) / length;
            }
            original_cursor = original_token_end;
            formatted_cursor = formatted_token_end;
        }
        Self::fill_gap(
            &mut boundaries,
            original_cursor..original_end,
            formatted_cursor..formatted_end,
        );
        boundaries[original_end] = formatted_end;
        boundaries
    }

    fn fill_gap(
        boundaries: &mut [usize],
        original: std::ops::Range<usize>,
        formatted: std::ops::Range<usize>,
    ) {
        let length = original.end - original.start;
        let formatted_length = formatted.end - formatted.start;
        for offset in 0..length {
            boundaries[original.start + offset] =
                formatted.start + offset * formatted_length / length.max(1);
        }
    }
}

struct RustTokens<'a> {
    text: &'a str,
    position: usize,
    tokens: Vec<LocatedToken>,
}

impl<'a> RustTokens<'a> {
    fn scan(text: &'a str) -> Vec<LocatedToken> {
        let mut scanner = Self {
            text,
            position: 0,
            tokens: Vec::new(),
        };
        while scanner.position < text.len() {
            scanner.step();
        }
        scanner.tokens
    }

    fn step(&mut self) {
        let character = self.current();
        if character.is_whitespace() {
            self.position += character.len_utf8();
            return;
        }
        if self.comment() {
            return;
        }
        if self.literal(character) {
            return;
        }
        if character.is_alphanumeric() || character == '_' {
            self.word();
        } else {
            self.punctuation();
        }
    }

    fn comment(&mut self) -> bool {
        let rest = &self.text[self.position..];
        if rest.starts_with("//") {
            self.line_comment();
            true
        } else if rest.starts_with("/*") {
            self.block_comment();
            true
        } else {
            false
        }
    }

    fn literal(&mut self, character: char) -> bool {
        match character {
            '"' => {
                self.string();
                true
            }
            '\'' => self.character(),
            'r' => self.raw(),
            _ => false,
        }
    }

    fn character(&mut self) -> bool {
        let Some(end) = self.char_literal_end(self.position) else {
            return false;
        };
        self.push(self.position, end);
        self.position = end;
        true
    }

    fn raw(&mut self) -> bool {
        let Some(hashes) = self.raw_string_hashes() else {
            return false;
        };
        self.raw_string(hashes);
        true
    }

    fn current(&self) -> char {
        self.text[self.position..]
            .chars()
            .next()
            .expect("the scanner stays inside the text")
    }

    fn word(&mut self) {
        let start = self.position;
        while self.position < self.text.len() {
            let character = self.current();
            if character.is_alphanumeric() || character == '_' {
                self.position += character.len_utf8();
            } else {
                break;
            }
        }
        self.push(start, self.position);
    }

    fn punctuation(&mut self) {
        let start = self.position;
        self.position += self.current().len_utf8();
        self.push(start, self.position);
    }

    fn string(&mut self) {
        let start = self.position;
        self.position += 1;
        let bytes = self.text.as_bytes();
        let mut escaped = false;
        while self.position < self.text.len() {
            let byte = bytes[self.position];
            self.position += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                break;
            }
        }
        self.push(start, self.position);
    }

    fn raw_string_hashes(&self) -> Option<usize> {
        let bytes = self.text.as_bytes();
        let mut index = self.position + 1;
        let mut hashes = 0;
        while bytes.get(index) == Some(&b'#') {
            hashes += 1;
            index += 1;
        }
        (bytes.get(index) == Some(&b'"')).then_some(hashes)
    }

    fn raw_string(&mut self, hashes: usize) {
        let start = self.position;
        let bytes = self.text.as_bytes();
        self.position += hashes + 2;
        while self.position < self.text.len() {
            if bytes[self.position] == b'"'
                && (1..=hashes).all(|offset| bytes.get(self.position + offset) == Some(&b'#'))
            {
                self.position += hashes + 1;
                break;
            }
            self.position += 1;
        }
        self.push(start, self.position);
    }

    fn char_literal_end(&self, position: usize) -> Option<usize> {
        let bytes = self.text.as_bytes();
        let mut index = position + 1;
        if bytes.get(index) != Some(&b'\\') {
            return self.plain_character_end(index);
        }
        index = self.escape_end(index + 1)?;
        (bytes.get(index) == Some(&b'\'')).then_some(index + 1)
    }

    fn plain_character_end(&self, index: usize) -> Option<usize> {
        let character = self.text[index..].chars().next()?;
        let next = index + character.len_utf8();
        (self.text.as_bytes().get(next) == Some(&b'\'')).then_some(next + 1)
    }

    fn escape_end(&self, index: usize) -> Option<usize> {
        match self.text.as_bytes().get(index)? {
            b'u' => self.unicode_escape_end(index + 1),
            b'x' => Some(index + 3),
            _ => Some(index + 1),
        }
    }

    fn unicode_escape_end(&self, index: usize) -> Option<usize> {
        let bytes = self.text.as_bytes();
        if bytes.get(index) != Some(&b'{') {
            return None;
        }
        let mut cursor = index + 1;
        while bytes.get(cursor).is_some_and(|byte| *byte != b'}') {
            cursor += 1;
        }
        (bytes.get(cursor) == Some(&b'}')).then_some(cursor + 1)
    }

    fn line_comment(&mut self) {
        while self.position < self.text.len() {
            let byte = self.text.as_bytes()[self.position];
            self.position += 1;
            if byte == b'\n' {
                break;
            }
        }
    }

    fn block_comment(&mut self) {
        self.position += 2;
        let mut depth = 1;
        while depth > 0 && self.position < self.text.len() {
            let rest = &self.text[self.position..];
            if rest.starts_with("/*") {
                depth += 1;
                self.position += 2;
            } else if rest.starts_with("*/") {
                depth -= 1;
                self.position += 2;
            } else {
                self.position += self.current().len_utf8();
            }
        }
    }

    fn push(&mut self, start: usize, end: usize) {
        self.tokens.push(LocatedToken {
            text: self.text[start..end].to_string(),
            start,
            end,
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn boundaries(original: &str, formatted: &str) -> Vec<usize> {
        TokenAlignment::new(original, formatted)
            .boundaries()
            .expect("the tokens align")
    }

    fn tokens(source: &str) -> Vec<String> {
        RustTokens::scan(source)
            .into_iter()
            .map(|token| token.text)
            .collect()
    }

    #[test]
    fn scans_words_and_punctuation() {
        assert_eq!(
            tokens("  let value = 1_i64;  "),
            ["let", "value", "=", "1_i64", ";"]
        );
    }

    #[test]
    fn ignores_line_and_nested_block_comments() {
        let source = "let x = 1; // line\n/* outer /* inner */ outer */ let y = 2;";
        assert_eq!(
            tokens(source),
            ["let", "x", "=", "1", ";", "let", "y", "=", "2", ";"]
        );
    }

    #[test]
    fn ignores_an_unterminated_block_comment() {
        assert_eq!(tokens("let x = 1; /* open"), ["let", "x", "=", "1", ";"]);
    }

    #[test]
    fn scans_a_string_with_an_escape() {
        let source = r#"let text = "a\"b";"#;
        assert_eq!(tokens(source), ["let", "text", "=", "\"a\\\"b\"", ";"]);
    }

    #[test]
    fn scans_a_string_with_unicode() {
        assert_eq!(
            tokens("let text = \"héllo\";"),
            ["let", "text", "=", "\"héllo\"", ";"]
        );
    }

    #[test]
    fn scans_a_raw_string_with_hashes() {
        let source = r##"let text = r#"a " b"#;"##;
        assert_eq!(tokens(source), ["let", "text", "=", "r#\"a \" b\"#", ";"]);
    }

    #[test]
    fn scans_a_raw_string_without_hashes() {
        let source = r#"let text = r"a";"#;
        assert_eq!(tokens(source), ["let", "text", "=", "r\"a\"", ";"]);
    }

    #[test]
    fn scans_character_literals() {
        let source =
            r#"let a = '\n'; let b = '\''; let c = '\u{1F600}'; let d = '\x41'; let e = '"';"#;
        let scanned = tokens(source);
        for literal in ["'\\n'", "'\\''", "'\\u{1F600}'", "'\\x41'", "'\"'"] {
            assert!(scanned.contains(&literal.to_string()), "missing {literal}");
        }
    }

    #[test]
    fn treats_a_lifetime_as_punctuation_and_a_word() {
        assert_eq!(tokens("'a"), ["'", "a"]);
    }

    #[test]
    fn scans_an_unterminated_escape_as_punctuation() {
        assert_eq!(tokens("'\\"), ["'", "\\"]);
        assert_eq!(tokens("'\\u"), ["'", "\\", "u"]);
    }

    #[test]
    fn aligns_tokens_across_an_insertion() {
        assert!(!boundaries("println!(\"x\")", "{ println!(\"x\") }").is_empty());
    }

    #[test]
    fn aligns_tokens_across_a_deletion() {
        assert!(!boundaries("{ println!(\"x\") }", "println!(\"x\")").is_empty());
    }

    #[test]
    fn reports_no_alignment_for_different_tokens() {
        assert!(
            TokenAlignment::new("first", "second")
                .boundaries()
                .is_none()
        );
    }

    #[test]
    fn reports_no_alignment_for_empty_tokens() {
        let alignment = TokenAlignment {
            original: Vec::new(),
            formatted: Vec::new(),
        };
        assert!(alignment.boundaries().is_none());
    }

    #[test]
    fn ignores_comments_in_both_texts() {
        let original = "let value = 1;\n// an original comment\n";
        let formatted = "let value = 1;\n// a formatted comment\n";
        let end = original.find(';').expect("the code holds a semicolon") + 1;
        assert_eq!(boundaries(original, formatted).len(), end + 1);
    }

    #[test]
    fn remaps_an_empty_span_map() {
        assert!(
            TokenAlignment::new("value", "value")
                .remap(&SpanMap::default())
                .is_some()
        );
    }

    #[test]
    fn keeps_a_character_literal_with_a_quote_whole() {
        assert!(!boundaries("let marker = '\"';", "let marker = '\"';").is_empty());
    }
}
