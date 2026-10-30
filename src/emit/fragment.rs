use crate::source::Span;

#[derive(Clone, Debug)]
pub enum Fragment {
    Text(String),
    Newline,
    Indented(Vec<Fragment>),
    Spanned(Span, Vec<Fragment>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precedence {
    Atom,
    Prefix,
    Operator,
    Block,
}

#[derive(Clone, Debug)]
pub struct Code {
    pub fragments: Vec<Fragment>,
    pub precedence: Precedence,
    pub bare_struct_literal: bool,
}

impl Code {
    pub fn atom(text: impl Into<String>) -> Self {
        Self::with(text, Precedence::Atom)
    }

    pub fn with(text: impl Into<String>, precedence: Precedence) -> Self {
        Self {
            fragments: vec![Fragment::Text(text.into())],
            precedence,
            bare_struct_literal: false,
        }
    }

    pub fn joined(parts: Vec<Code>, precedence: Precedence) -> Self {
        Self {
            fragments: parts.into_iter().flat_map(|part| part.fragments).collect(),
            precedence,
            bare_struct_literal: false,
        }
    }

    pub fn joined_outside_delimiters(parts: Vec<Code>, precedence: Precedence) -> Self {
        let bare_struct_literal = parts.iter().any(|part| part.bare_struct_literal);
        Self {
            bare_struct_literal,
            ..Self::joined(parts, precedence)
        }
    }

    pub fn struct_literal(parts: Vec<Code>) -> Self {
        Self {
            bare_struct_literal: true,
            ..Self::joined(parts, Precedence::Atom)
        }
    }

    pub fn block(fragments: Vec<Fragment>) -> Self {
        Self {
            fragments,
            precedence: Precedence::Block,
            bare_struct_literal: false,
        }
    }

    pub fn spanned(self, span: Span) -> Self {
        Self {
            fragments: vec![Fragment::Spanned(span, self.fragments)],
            ..self
        }
    }

    pub fn parenthesized(self) -> Self {
        Self::joined(
            vec![Code::atom("("), self, Code::atom(")")],
            Precedence::Atom,
        )
    }

    pub fn into_head(self) -> Self {
        if self.bare_struct_literal {
            self.parenthesized()
        } else {
            self
        }
    }

    pub fn is_atomic(&self) -> bool {
        self.precedence == Precedence::Atom
    }

    pub fn is_plain_name(&self) -> bool {
        self.is_atomic()
            && self
                .text()
                .chars()
                .all(|character| character.is_alphanumeric() || "_:#".contains(character))
    }

    pub fn text(&self) -> String {
        Self::text_of(&self.fragments)
    }

    pub fn text_of(fragments: &[Fragment]) -> String {
        let mut text = String::new();
        Self::collect_text(fragments, &mut text);
        text
    }

    fn collect_text(fragments: &[Fragment], text: &mut String) {
        for fragment in fragments {
            match fragment {
                Fragment::Text(part) => text.push_str(part),
                Fragment::Newline => text.push('\n'),
                Fragment::Indented(inner) | Fragment::Spanned(_, inner) => {
                    Self::collect_text(inner, text)
                }
            }
        }
    }
}
