use crate::source::Span;

#[derive(Clone, Debug)]
pub enum Fragment {
    Text(String),
    Newline,
    Indented(Vec<Fragment>),
    Spanned(Span, Vec<Fragment>),
    StructLiteral(Vec<Fragment>),
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
}

impl Code {
    const PREFIX_OPERATORS: [&str; 6] = ["", "!", "&", "&mut ", "*", "-"];

    pub fn atom(text: impl Into<String>) -> Self {
        Self::with(text, Precedence::Atom)
    }

    pub fn with(text: impl Into<String>, precedence: Precedence) -> Self {
        Self {
            fragments: vec![Fragment::Text(text.into())],
            precedence,
        }
    }

    pub fn joined(parts: Vec<Code>, precedence: Precedence) -> Self {
        Self {
            fragments: parts.into_iter().flat_map(|part| part.fragments).collect(),
            precedence,
        }
    }

    pub fn struct_literal(parts: Vec<Code>) -> Self {
        Self {
            fragments: vec![Fragment::StructLiteral(
                Self::joined(parts, Precedence::Atom).fragments,
            )],
            precedence: Precedence::Atom,
        }
    }

    pub fn spanned(self, span: Span) -> Self {
        Self {
            fragments: vec![Fragment::Spanned(span, self.fragments)],
            precedence: self.precedence,
        }
    }

    pub fn parenthesized(self) -> Self {
        Self::joined(
            vec![Code::atom("("), self, Code::atom(")")],
            Precedence::Atom,
        )
    }

    pub fn into_head(self) -> Self {
        if self.starts_with_struct_literal() {
            self.parenthesized()
        } else {
            self
        }
    }

    fn starts_with_struct_literal(&self) -> bool {
        Self::left_edge_is_struct_literal(&self.fragments) == Some(true)
    }

    fn left_edge_is_struct_literal(fragments: &[Fragment]) -> Option<bool> {
        fragments.iter().find_map(|fragment| match fragment {
            Fragment::StructLiteral(_) => Some(true),
            Fragment::Spanned(_, inner) => Self::left_edge_is_struct_literal(inner),
            Fragment::Text(text) if Self::PREFIX_OPERATORS.contains(&text.as_str()) => None,
            _ => Some(false),
        })
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
                Fragment::Indented(inner)
                | Fragment::Spanned(_, inner)
                | Fragment::StructLiteral(inner) => Self::collect_text(inner, text),
            }
        }
    }
}
