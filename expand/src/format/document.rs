use super::block::Block;
use super::layout::{LaidOut, LayoutEngine};
use super::syntax_tree::{Comment, Entry, SyntaxNode, SyntaxTree};
use crate::core::{DefinitionForm, KernelForm};

pub struct DocumentPrinter<'e, 's> {
    engine: &'e mut LayoutEngine<'s>,
    output: String,
    previous_group: Option<ImportGroup>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ImportGroup {
    Use,
    Export,
}

impl<'e, 's> DocumentPrinter<'e, 's> {
    const LINE_BREAK: &'static str = "\n";
    const BLANK_LINE: &'static str = "\n\n";

    pub fn new(engine: &'e mut LayoutEngine<'s>) -> Self {
        Self {
            engine,
            output: String::new(),
            previous_group: None,
        }
    }

    pub fn print(mut self, tree: &SyntaxTree) -> String {
        for entry in &tree.entries {
            self.print_entry(entry);
        }
        self.print_closing_comments(&tree.closing_comments);
        if !self.output.is_empty() {
            self.output.push_str(Self::LINE_BREAK);
        }
        self.output
    }

    fn print_entry(&mut self, entry: &Entry) {
        let mut block: Block = self.engine.node(&entry.node, 0, 0);
        if let Some(comment) = &entry.trailing_comment {
            block.add_trailing_comment(comment);
        }
        let group = ImportGroup::of(entry, &block);
        let joins_group =
            group.is_some() && group == self.previous_group && entry.leading_comments.is_empty();
        self.separate(joins_group);
        self.print_comments(&entry.leading_comments);
        if !entry.leading_comments.is_empty() && entry.blank_before_node {
            self.output.push_str(Self::LINE_BREAK);
        }
        block.render_into(&mut self.output);
        self.previous_group = group;
    }

    fn separate(&mut self, joins_group: bool) {
        if self.output.is_empty() {
            return;
        }
        let separator = if joins_group {
            Self::LINE_BREAK
        } else {
            Self::BLANK_LINE
        };
        self.output.push_str(separator);
    }

    fn print_comments(&mut self, comments: &[Comment]) {
        for (position, comment) in comments.iter().enumerate() {
            if position > 0 && comment.blank_before {
                self.output.push_str(Self::LINE_BREAK);
            }
            self.output.push_str(&comment.text);
            self.output.push_str(Self::LINE_BREAK);
        }
    }

    fn print_closing_comments(&mut self, comments: &[Comment]) {
        if comments.is_empty() {
            return;
        }
        self.separate(false);
        self.print_comments(comments);
        self.output.pop();
    }
}

impl ImportGroup {
    fn of(entry: &Entry, block: &Block) -> Option<Self> {
        let SyntaxNode::List(list) = &entry.node else {
            return None;
        };
        let head = list.entries.first()?.node.symbol_name()?;
        (block.line_count() == 1).then_some(())?;
        [Self::Use, Self::Export]
            .into_iter()
            .find(|group| group.form_name() == head)
    }

    fn form_name(self) -> &'static str {
        let form = match self {
            Self::Use => DefinitionForm::Use,
            Self::Export => DefinitionForm::Export,
        };
        KernelForm::Definition(form).name()
    }
}
