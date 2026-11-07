use std::ops::Range;

use crate::reader::{Token, TokenKind, TokenScan};
use crate::stack::DeepRecursion;

pub struct SyntaxTree {
    pub entries: Vec<Entry>,
    pub closing_comments: Vec<Comment>,
}

pub struct Entry {
    pub leading_comments: Vec<Comment>,
    pub blank_before_node: bool,
    pub node: SyntaxNode,
    pub trailing_comment: Option<String>,
}

pub struct Comment {
    pub text: String,
    pub blank_before: bool,
}

pub enum SyntaxNode {
    Atom(Atom),
    List(ListNode),
}

pub struct Atom {
    pub text: String,
    pub kind: TokenKind,
    pub written_column: usize,
}

pub struct ListNode {
    pub id: usize,
    pub entries: Vec<Entry>,
    pub closing_comments: Vec<Comment>,
    pub nested_lists: usize,
    pub ends_with_comment: bool,
}

impl SyntaxTree {
    pub fn build(text: &str) -> Self {
        let mut builder = TreeBuilder::new(text);
        let (entries, closing_comments) = builder.entries();
        Self {
            entries,
            closing_comments,
        }
    }
}

impl SyntaxNode {
    pub fn as_atom(&self) -> Option<&Atom> {
        match self {
            SyntaxNode::Atom(atom) => Some(atom),
            SyntaxNode::List(_) => None,
        }
    }

    pub fn as_list(&self) -> Option<&ListNode> {
        match self {
            SyntaxNode::List(list) => Some(list),
            SyntaxNode::Atom(_) => None,
        }
    }

    pub fn ends_with_comment(&self) -> bool {
        match self {
            SyntaxNode::List(list) => list.ends_with_comment,
            SyntaxNode::Atom(_) => false,
        }
    }

    pub fn symbol_name(&self) -> Option<&str> {
        self.as_atom()
            .filter(|atom| atom.kind == TokenKind::Symbol)
            .map(|atom| atom.text.as_str())
    }
}

impl Atom {
    pub fn is_multi_line(&self) -> bool {
        self.text.contains('\n')
    }

    pub fn is_string(&self) -> bool {
        self.kind == TokenKind::String
    }

    pub fn docstring_shift(&self, column: usize) -> isize {
        let shift = column as isize - self.written_column as isize;
        let least_indentation = self
            .text
            .split('\n')
            .skip(1)
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.len() - line.trim_start_matches(' ').len())
            .min()
            .unwrap_or(0);
        shift.max(-(least_indentation as isize))
    }
}

impl Entry {
    pub fn is_interrupted(&self) -> bool {
        !self.leading_comments.is_empty() || self.blank_before_node
    }

    pub fn interrupts_line(&self, index: usize) -> bool {
        !self.leading_comments.is_empty() || self.keeps_blank_line(index)
    }

    fn ends_with_comment(&self) -> bool {
        self.trailing_comment.is_some() || self.node.ends_with_comment()
    }

    fn lists_within(&self) -> usize {
        match &self.node {
            SyntaxNode::List(list) => 1 + list.nested_lists,
            SyntaxNode::Atom(_) => 0,
        }
    }

    pub fn keeps_blank_line(&self, index: usize) -> bool {
        index > 0 && self.blank_before_node
    }
}

struct TreeBuilder<'a> {
    text: &'a str,
    tokens: Vec<Token>,
    position: usize,
    next_list_id: usize,
}

impl<'a> TreeBuilder<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            tokens: TokenScan::new(text).collect(),
            position: 0,
            next_list_id: 0,
        }
    }

    fn entries(&mut self) -> (Vec<Entry>, Vec<Comment>) {
        let mut entries: Vec<Entry> = Vec::new();
        let mut pending = Vec::new();
        while let Some(token) = self.peek().filter(|token| token.kind != TokenKind::Close) {
            if token.kind == TokenKind::Comment {
                self.add_comment(&mut entries, &mut pending);
            } else {
                entries.push(self.entry(std::mem::take(&mut pending)));
            }
        }
        (entries, pending)
    }

    fn add_comment(&mut self, entries: &mut [Entry], pending: &mut Vec<Comment>) {
        let trails_entry = self.continues_line();
        let open_entry = entries
            .last_mut()
            .filter(|entry| trails_entry && !entry.node.ends_with_comment());
        let text = self.token_text().to_string();
        let blank_before = self.follows_blank_line();
        self.position += 1;
        match open_entry {
            Some(entry) => entry.trailing_comment = Some(text),
            None => pending.push(Comment { text, blank_before }),
        }
    }

    fn entry(&mut self, leading_comments: Vec<Comment>) -> Entry {
        let blank_before_node = self.follows_blank_line();
        Entry {
            leading_comments,
            blank_before_node,
            node: DeepRecursion::guard(|| self.node()),
            trailing_comment: None,
        }
    }

    fn node(&mut self) -> SyntaxNode {
        let token = self.peek().expect("an entry starts at a token");
        self.position += 1;
        match token.kind {
            TokenKind::Open => SyntaxNode::List(self.list()),
            kind => SyntaxNode::Atom(self.atom(kind, token.range)),
        }
    }

    fn list(&mut self) -> ListNode {
        let id = self.next_list_id;
        self.next_list_id += 1;
        let (entries, closing_comments) = self.entries();
        self.position += 1;
        ListNode {
            id,
            nested_lists: entries.iter().map(Entry::lists_within).sum(),
            ends_with_comment: closing_comments.is_empty()
                && entries.last().is_some_and(Entry::ends_with_comment),
            entries,
            closing_comments,
        }
    }

    fn atom(&self, kind: TokenKind, range: Range<usize>) -> Atom {
        let line_start = self.text[..range.start].rfind('\n').map_or(0, |at| at + 1);
        Atom {
            text: self.text[range.clone()].to_string(),
            kind,
            written_column: self.text[line_start..range.start].chars().count(),
        }
    }

    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.position).cloned()
    }

    fn token_text(&self) -> &'a str {
        &self.text[self.tokens[self.position].range.clone()]
    }

    fn continues_line(&self) -> bool {
        self.position > 0 && self.newlines_before() == 0
    }

    fn follows_blank_line(&self) -> bool {
        self.position > 0 && self.newlines_before() > 1
    }

    fn newlines_before(&self) -> usize {
        let start = self.tokens[self.position].range.start;
        let previous_end = self.tokens[self.position - 1].range.end;
        self.text[previous_end..start].matches('\n').count()
    }
}

#[cfg(test)]
mod tests {
    use super::{SyntaxNode, SyntaxTree};

    struct TreeFixture {
        tree: SyntaxTree,
    }

    impl TreeFixture {
        fn of(text: &str) -> Self {
            Self {
                tree: SyntaxTree::build(text),
            }
        }

        fn list_entries(&self) -> &[super::Entry] {
            match &self.tree.entries[0].node {
                SyntaxNode::List(list) => &list.entries,
                SyntaxNode::Atom(_) => panic!("the first form is a list"),
            }
        }
    }

    #[test]
    fn a_comment_on_the_line_of_a_form_trails_it() {
        let fixture = TreeFixture::of("(a b ; why\n c)");
        let entries = fixture.list_entries();
        assert_eq!(entries[1].trailing_comment.as_deref(), Some("; why"));
        assert!(entries[2].leading_comments.is_empty());
    }

    #[test]
    fn a_comment_on_its_own_line_leads_the_next_form() {
        let fixture = TreeFixture::of("(a\n ; note\n b)");
        let entries = fixture.list_entries();
        assert_eq!(entries[0].trailing_comment, None);
        assert_eq!(entries[1].leading_comments[0].text, "; note");
    }

    #[test]
    fn a_comment_before_the_close_stays_inside_the_list() {
        let fixture = TreeFixture::of("(a\n ; last\n)");
        let SyntaxNode::List(list) = &fixture.tree.entries[0].node else {
            panic!("the first form is a list");
        };
        assert_eq!(list.closing_comments[0].text, "; last");
    }

    #[test]
    fn a_blank_line_between_forms_is_kept_and_a_single_break_is_not() {
        let fixture = TreeFixture::of("(a\n b\n\n c)");
        let entries = fixture.list_entries();
        assert!(!entries[1].blank_before_node);
        assert!(entries[2].blank_before_node);
    }

    #[test]
    fn an_atom_keeps_its_text_and_its_written_column() {
        let fixture = TreeFixture::of("(a\n   1_000)");
        let atom = fixture.list_entries()[1].node.as_atom().expect("an atom");
        assert_eq!((atom.text.as_str(), atom.written_column), ("1_000", 3));
    }
}
