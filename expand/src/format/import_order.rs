use super::syntax_tree::{Entry, ListNode, SyntaxNode, SyntaxTree};
use crate::core::{ControlForm, DefinitionForm, ExpressionForm, KernelForm};
use crate::stack::DeepRecursion;

pub struct ImportList;

pub struct ImportSorter;

impl ImportList {
    pub fn names_start(head: &str) -> Option<usize> {
        [(DefinitionForm::Use, 2), (DefinitionForm::Export, 1)]
            .into_iter()
            .find(|(form, _)| KernelForm::Definition(*form).name() == head)
            .map(|(_, start)| start)
    }

    pub fn splices(head: &str) -> bool {
        KernelForm::Expression(ExpressionForm::Control(ControlForm::Do)).name() == head
    }
}

impl ImportSorter {
    pub fn sort(tree: &mut SyntaxTree) {
        Self::sort_entries(&mut tree.entries);
    }

    fn sort_entries(entries: &mut [Entry]) {
        for entry in entries {
            if let SyntaxNode::List(list) = &mut entry.node {
                DeepRecursion::guard(|| Self::sort_top_level(list));
            }
        }
    }

    fn sort_top_level(list: &mut ListNode) {
        let head = list
            .entries
            .first()
            .and_then(|head| head.node.symbol_name());
        let splices = head.is_some_and(ImportList::splices);
        let names_start = head.and_then(ImportList::names_start);
        if splices {
            Self::sort_entries(&mut list.entries);
        } else if let Some(start) = names_start {
            Self::sort_names(list, start);
        }
    }

    fn sort_names(list: &mut ListNode, start: usize) {
        let closed_without_comment = list.closing_comments.is_empty();
        if let Some(names) = list.entries.get_mut(start..)
            && closed_without_comment
            && names.iter().all(Self::is_movable_name)
        {
            names.sort_by(|left, right| Self::name_text(left).cmp(Self::name_text(right)));
        }
    }

    fn is_movable_name(entry: &Entry) -> bool {
        entry.node.as_atom().is_some()
            && !entry.is_interrupted()
            && entry.trailing_comment.is_none()
    }

    fn name_text(entry: &Entry) -> &str {
        entry.node.as_atom().map_or("", |atom| atom.text.as_str())
    }
}
