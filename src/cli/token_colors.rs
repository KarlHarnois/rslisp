use anstream::ColorChoice;
use anstyle::{AnsiColor, Effects, Style};

use crate::core::Keyword;
use crate::reader::{Token, TokenKind, TokenScan};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenColors {
    Painted,
    Plain,
}

impl TokenColors {
    pub const STRING_STYLE: Style = AnsiColor::Green.on_default();
    pub const NUMBER_STYLE: Style = AnsiColor::Yellow.on_default();
    pub const KEYWORD_STYLE: Style = AnsiColor::Cyan.on_default();
    pub const COMMENT_STYLE: Style = Style::new().effects(Effects::DIMMED);

    pub fn deciding(choice: ColorChoice) -> Self {
        match choice {
            ColorChoice::Never => Self::Plain,
            _ => Self::Painted,
        }
    }

    pub fn paint(self, text: &str) -> String {
        match self {
            Self::Painted => Self::painted(text),
            Self::Plain => text.to_string(),
        }
    }

    fn painted(text: &str) -> String {
        let mut painted = String::with_capacity(text.len());
        let mut written = 0;
        for token in TokenScan::new(text) {
            painted.push_str(&text[written..token.range.start]);
            let style = Self::style_of(text, &token);
            painted.push_str(&format!("{style}{}{style:#}", &text[token.range.clone()]));
            written = token.range.end;
        }
        painted.push_str(&text[written..]);
        painted
    }

    fn style_of(text: &str, token: &Token) -> Style {
        match token.kind {
            TokenKind::String => Self::STRING_STYLE,
            TokenKind::Number => Self::NUMBER_STYLE,
            TokenKind::Comment => Self::COMMENT_STYLE,
            TokenKind::Symbol => Self::symbol_style(&text[token.range.clone()]),
            TokenKind::Open | TokenKind::Close | TokenKind::Reserved => Style::new(),
        }
    }

    fn symbol_style(symbol: &str) -> Style {
        match Keyword::field_name(symbol) {
            Some(_) => Self::KEYWORD_STYLE,
            None => Style::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PaintFixture;

    impl PaintFixture {
        fn painted(text: &str) -> String {
            TokenColors::Painted.paint(text)
        }

        fn styled(style: Style, text: &str) -> String {
            format!("{style}{text}{style:#}")
        }
    }

    #[test]
    fn each_kind_of_token_takes_its_color() {
        assert_eq!(
            PaintFixture::painted("(f :x 1 \"s\") ; c"),
            format!(
                "(f {} {} {}) {}",
                PaintFixture::styled(TokenColors::KEYWORD_STYLE, ":x"),
                PaintFixture::styled(TokenColors::NUMBER_STYLE, "1"),
                PaintFixture::styled(TokenColors::STRING_STYLE, "\"s\""),
                PaintFixture::styled(TokenColors::COMMENT_STYLE, "; c"),
            )
        );
    }

    #[test]
    fn a_partial_text_colors_its_unclosed_string() {
        assert_eq!(
            PaintFixture::painted("(defn f \"abc"),
            format!(
                "(defn f {}",
                PaintFixture::styled(TokenColors::STRING_STYLE, "\"abc")
            )
        );
    }

    #[test]
    fn a_lone_keyword_marker_stays_plain() {
        assert_eq!(PaintFixture::painted(": [x]"), ": [x]");
    }

    #[test]
    fn plain_colors_leave_the_text_as_it_is() {
        assert_eq!(TokenColors::Plain.paint("(f :x 1)"), "(f :x 1)");
    }

    #[test]
    fn only_a_decision_of_never_leaves_the_text_plain() {
        assert_eq!(
            TokenColors::deciding(ColorChoice::Never),
            TokenColors::Plain
        );
        assert_eq!(
            TokenColors::deciding(ColorChoice::AlwaysAnsi),
            TokenColors::Painted
        );
    }
}
