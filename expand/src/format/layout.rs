use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::block::Block;
use super::list_plan::{Arrangement, LaterIndent, ListPlan, Unit};
use super::measure::Measure;
use super::shapes::ShapeCatalog;
use super::syntax_tree::{Entry, ListNode, SyntaxNode};
use crate::stack::DeepRecursion;

pub trait LaidOut: Sized {
    fn text(text: &str, column: usize) -> Self;
    fn shifted_string(text: &str, shift: isize, column: usize) -> Self;
    fn list(engine: &mut LayoutEngine<'_>, list: &ListNode, column: usize, trailing: usize)
    -> Self;
    fn line_count(&self) -> usize;
    fn excuse_overflow(&mut self, own_width: usize, trailing: usize);
    fn end_column(&self) -> usize;
    fn ends_with_comment(&self) -> bool;
    fn append(&mut self, text: &str);
    fn join(&mut self, separator: &str, other: Self);
    fn push_indented(&mut self, indent: usize, other: Self);
    fn push_blank(&mut self);
    fn add_trailing_comment(&mut self, comment: &str);
}

pub struct LayoutEngine<'s> {
    shapes: &'s ShapeCatalog,
    plans: FxHashMap<usize, Rc<ListPlan>>,
    one_line_texts: FxHashMap<usize, Option<Rc<str>>>,
    choices: FxHashMap<Placement, Choice>,
    arms: FxHashSet<usize>,
    binding_lists: FxHashSet<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Placement {
    list: usize,
    column: usize,
    trailing: usize,
}

#[derive(Clone, Copy)]
struct Choice {
    measure: Measure,
    layout: ChosenLayout,
}

#[derive(Clone, Copy)]
enum ChosenLayout {
    OneLine,
    Arranged(Arrangement),
}

struct ListAssembly<'l> {
    list: &'l ListNode,
    plan: &'l ListPlan,
    column: usize,
    trailing: usize,
}

enum Position {
    FirstLine,
    Filled,
    OwnLine,
}

struct AssemblyState<L> {
    laid_out: L,
    later_indent: Option<usize>,
    on_first_line: bool,
    run_open: bool,
}

impl<'s> LayoutEngine<'s> {
    pub const SHORT_LIST_LIMIT: usize = 40;
    pub const SPARSE_LIST_LIMIT: usize = 60;
    pub const SPARSE_LIST_NESTED_LISTS: usize = 2;

    pub fn new(shapes: &'s ShapeCatalog) -> Self {
        Self {
            shapes,
            plans: FxHashMap::default(),
            one_line_texts: FxHashMap::default(),
            choices: FxHashMap::default(),
            arms: FxHashSet::default(),
            binding_lists: FxHashSet::default(),
        }
    }

    pub fn node<L: LaidOut>(&mut self, node: &SyntaxNode, column: usize, trailing: usize) -> L {
        match node {
            SyntaxNode::Atom(atom) => L::text(&atom.text, column),
            SyntaxNode::List(list) => L::list(self, list, column, trailing),
        }
    }

    pub fn mark_arm(&mut self, node: &SyntaxNode) {
        if let SyntaxNode::List(list) = node {
            self.arms.insert(list.id);
        }
    }

    #[cfg(test)]
    fn placement_count(&self) -> usize {
        self.choices.len()
    }

    pub fn measure_list(&mut self, list: &ListNode, column: usize, trailing: usize) -> Measure {
        self.choice(list, column, trailing).measure
    }

    pub fn render_list(&mut self, list: &ListNode, column: usize, trailing: usize) -> Block {
        match self.choice(list, column, trailing).layout {
            ChosenLayout::OneLine => {
                let text = self
                    .one_line_text(list)
                    .expect("a one-line layout has its text");
                Block::text(&text, column)
            }
            ChosenLayout::Arranged(arrangement) => {
                let plan = self.plan(list);
                let assembly = ListAssembly::new(list, &plan, column, trailing);
                DeepRecursion::guard(|| assembly.assemble(self, arrangement))
            }
        }
    }

    fn choice(&mut self, list: &ListNode, column: usize, trailing: usize) -> Choice {
        let placement = Placement {
            list: list.id,
            column,
            trailing,
        };
        if let Some(choice) = self.choices.get(&placement) {
            return *choice;
        }
        let choice = DeepRecursion::guard(|| self.fresh_choice(list, column, trailing));
        self.choices.insert(placement, choice);
        choice
    }

    fn fresh_choice(&mut self, list: &ListNode, column: usize, trailing: usize) -> Choice {
        if let Some(text) = self.fitting_one_line(list, column, trailing) {
            return Choice {
                measure: Measure::text(&text, column),
                layout: ChosenLayout::OneLine,
            };
        }
        let plan = self.plan(list);
        let assembly = ListAssembly::new(list, &plan, column, trailing);
        plan.arrangements_at(column)
            .iter()
            .map(|arrangement| Choice {
                measure: assembly.assemble(self, *arrangement),
                layout: ChosenLayout::Arranged(*arrangement),
            })
            .min_by_key(|choice| {
                (
                    choice.measure.avoidable_overflow(trailing),
                    choice.measure.line_count(),
                )
            })
            .expect("every list has an arrangement")
    }

    fn fitting_one_line(
        &mut self,
        list: &ListNode,
        column: usize,
        trailing: usize,
    ) -> Option<Rc<str>> {
        let text = self.one_line_text(list)?;
        let length = text.chars().count();
        let fits = self.is_short(list, length) && column + length + trailing <= Block::WIDTH;
        fits.then_some(text)
    }

    fn is_short(&self, list: &ListNode, length: usize) -> bool {
        let sparse = length <= Self::SPARSE_LIST_LIMIT
            && list.nested_lists <= Self::SPARSE_LIST_NESTED_LISTS;
        let short_arm = length <= Block::FILL_WIDTH && self.arms.contains(&list.id);
        length <= Self::SHORT_LIST_LIMIT || sparse || short_arm
    }

    fn one_line_text(&mut self, list: &ListNode) -> Option<Rc<str>> {
        if let Some(text) = self.one_line_texts.get(&list.id) {
            return text.clone();
        }
        let text = DeepRecursion::guard(|| self.fresh_one_line_text(list));
        self.one_line_texts.insert(list.id, text.clone());
        text
    }

    fn fresh_one_line_text(&mut self, list: &ListNode) -> Option<Rc<str>> {
        let allowed = list.closing_comments.is_empty() && self.plan(list).allows_one_line();
        allowed.then_some(())?;
        let parts = list
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| self.one_line_entry(entry, index))
            .collect::<Option<Vec<Rc<str>>>>()?;
        Some(format!("({})", parts.join(" ")).into())
    }

    fn one_line_entry(&mut self, entry: &Entry, index: usize) -> Option<Rc<str>> {
        let breaks = entry.trailing_comment.is_some()
            || !entry.leading_comments.is_empty()
            || entry.keeps_blank_line(index);
        (!breaks).then_some(())?;
        match &entry.node {
            SyntaxNode::Atom(atom) if !atom.is_multi_line() => Some(atom.text.as_str().into()),
            SyntaxNode::Atom(_) => None,
            SyntaxNode::List(list) => self.one_line_text(list),
        }
    }

    fn plan(&mut self, list: &ListNode) -> Rc<ListPlan> {
        if let Some(plan) = self.plans.get(&list.id) {
            return Rc::clone(plan);
        }
        let plan = Rc::new(self.fresh_plan(list));
        self.mark_binding_list(list, &plan);
        self.plans.insert(list.id, Rc::clone(&plan));
        plan
    }

    fn fresh_plan(&self, list: &ListNode) -> ListPlan {
        if self.binding_lists.contains(&list.id) {
            ListPlan::binding_list(list)
        } else {
            ListPlan::new(list, self.shapes)
        }
    }

    fn mark_binding_list(&mut self, list: &ListNode, plan: &ListPlan) {
        let binding_list = plan
            .binding_list_entry
            .and_then(|entry| list.entries.get(entry))
            .and_then(|entry| entry.node.as_list());
        if let Some(binding_list) = binding_list {
            self.binding_lists.insert(binding_list.id);
        }
    }
}

impl<'l> ListAssembly<'l> {
    const OPEN: &'static str = "(";
    const CLOSE: &'static str = ")";
    const SEPARATOR: &'static str = " ";

    fn new(list: &'l ListNode, plan: &'l ListPlan, column: usize, trailing: usize) -> Self {
        Self {
            list,
            plan,
            column,
            trailing,
        }
    }

    fn assemble<L: LaidOut>(&self, engine: &mut LayoutEngine<'_>, arrangement: Arrangement) -> L {
        let mut state = AssemblyState {
            laid_out: L::text(Self::OPEN, self.column),
            later_indent: match arrangement.later_indent {
                LaterIndent::Past(indent) => Some(self.column + indent),
                LaterIndent::UnderLastOnFirstLine => None,
            },
            on_first_line: true,
            run_open: false,
        };
        for (index, unit) in self.plan.units.iter().enumerate() {
            self.place(engine, &mut state, arrangement, index, *unit);
        }
        self.close(&mut state);
        state.laid_out
    }

    fn place<L: LaidOut>(
        &self,
        engine: &mut LayoutEngine<'_>,
        state: &mut AssemblyState<L>,
        arrangement: Arrangement,
        index: usize,
        unit: Unit,
    ) {
        let continues_line = self.continues_line(state, index, unit);
        state.on_first_line &= index < arrangement.first_line_units && continues_line;
        match self.position(state, index, continues_line) {
            Position::FirstLine => self.join(engine, state, arrangement, index, unit),
            Position::Filled => self.fill(engine, state, index, unit),
            Position::OwnLine => self.push_on_own_line(engine, state, index, unit),
        }
        self.end_unit(state, arrangement, index, unit);
    }

    fn continues_line<L: LaidOut>(
        &self,
        state: &AssemblyState<L>,
        index: usize,
        unit: Unit,
    ) -> bool {
        let interrupted = unit
            .entries()
            .any(|entry| self.list.entries[entry].interrupts_line(index));
        !state.laid_out.ends_with_comment() && !interrupted
    }

    fn position<L: LaidOut>(
        &self,
        state: &AssemblyState<L>,
        index: usize,
        continues_line: bool,
    ) -> Position {
        if state.on_first_line {
            Position::FirstLine
        } else if continues_line && state.run_open && self.plan.fillable_units[index] {
            Position::Filled
        } else {
            Position::OwnLine
        }
    }

    fn end_unit<L: LaidOut>(
        &self,
        state: &mut AssemblyState<L>,
        arrangement: Arrangement,
        index: usize,
        unit: Unit,
    ) {
        for entry in unit.entries() {
            if let Some(comment) = &self.list.entries[entry].trailing_comment {
                state.laid_out.add_trailing_comment(comment);
            }
        }
        state.run_open =
            self.plan.fillable_units[index] && Self::opens_run(state.on_first_line, arrangement);
    }

    fn opens_run(on_first_line: bool, arrangement: Arrangement) -> bool {
        let aligned = arrangement.later_indent == LaterIndent::UnderLastOnFirstLine;
        !on_first_line || aligned
    }

    fn join<L: LaidOut>(
        &self,
        engine: &mut LayoutEngine<'_>,
        state: &mut AssemblyState<L>,
        arrangement: Arrangement,
        index: usize,
        unit: Unit,
    ) {
        let separator = if index == 0 { "" } else { Self::SEPARATOR };
        let start = state.laid_out.end_column() + separator.len();
        if index + 1 == arrangement.first_line_units {
            state.later_indent.get_or_insert(start);
        }
        let unit_layout = self.unit(engine, index, unit, start);
        state.laid_out.join(separator, unit_layout);
    }

    fn fill<L: LaidOut>(
        &self,
        engine: &mut LayoutEngine<'_>,
        state: &mut AssemblyState<L>,
        index: usize,
        unit: Unit,
    ) {
        let start = state.laid_out.end_column() + Self::SEPARATOR.len();
        let unit_layout: L = self.unit(engine, index, unit, start);
        let end = unit_layout.end_column() + self.unit_trailing(index);
        if end <= Block::FILL_WIDTH {
            state.laid_out.join(Self::SEPARATOR, unit_layout);
        } else {
            self.push_on_own_line(engine, state, index, unit);
        }
    }

    fn push_on_own_line<L: LaidOut>(
        &self,
        engine: &mut LayoutEngine<'_>,
        state: &mut AssemblyState<L>,
        index: usize,
        unit: Unit,
    ) {
        let indent = state.indent(self.column);
        let comments = unit
            .entries()
            .flat_map(|entry| &self.list.entries[entry].leading_comments);
        for (position, comment) in comments.enumerate() {
            if comment.blank_before && (index > 0 || position > 0) {
                state.laid_out.push_blank();
            }
            state
                .laid_out
                .push_indented(indent, L::text(&comment.text, indent));
        }
        let entry = &self.list.entries[unit.first_entry()];
        if entry.keeps_blank_line(index) {
            state.laid_out.push_blank();
        }
        let unit_layout = self.unit(engine, index, unit, indent);
        state.laid_out.push_indented(indent, unit_layout);
    }

    fn unit<L: LaidOut>(
        &self,
        engine: &mut LayoutEngine<'_>,
        index: usize,
        unit: Unit,
        start: usize,
    ) -> L {
        let trailing = self.unit_trailing(index);
        match unit {
            Unit::Single(entry) => self.entry_layout(engine, entry, start, trailing),
            Unit::Pair(keyword, value) => {
                let mut laid_out: L = self.entry_layout(engine, keyword, start, 0);
                let value_start = laid_out.end_column() + Self::SEPARATOR.len();
                let value_layout = self.pair_value_layout(engine, value, value_start, trailing);
                laid_out.join(Self::SEPARATOR, value_layout);
                laid_out
            }
        }
    }

    fn entry_layout<L: LaidOut>(
        &self,
        engine: &mut LayoutEngine<'_>,
        entry: usize,
        start: usize,
        trailing: usize,
    ) -> L {
        let node = &self.list.entries[entry].node;
        match node.as_atom() {
            Some(atom) if self.plan.docstring_entry == Some(entry) => {
                L::shifted_string(&atom.text, atom.docstring_shift(start), start)
            }
            Some(atom) if self.fits_nowhere(&atom.text, trailing) => {
                Self::excused_atom(&atom.text, start, trailing)
            }
            _ if self.plan.is_arm(entry) => {
                engine.mark_arm(node);
                engine.node(node, start, trailing)
            }
            _ => engine.node(node, start, trailing),
        }
    }

    fn pair_value_layout<L: LaidOut>(
        &self,
        engine: &mut LayoutEngine<'_>,
        value: usize,
        start: usize,
        trailing: usize,
    ) -> L {
        let node = &self.list.entries[value].node;
        match node.as_atom() {
            Some(atom) if self.fits_nowhere(&atom.text, trailing) => {
                Self::excused_atom(&atom.text, start, trailing)
            }
            _ => engine.node(node, start, trailing),
        }
    }

    fn excused_atom<L: LaidOut>(text: &str, start: usize, trailing: usize) -> L {
        let mut laid_out = L::text(text, start);
        laid_out.excuse_overflow(text.chars().count(), trailing);
        laid_out
    }

    fn fits_nowhere(&self, text: &str, trailing: usize) -> bool {
        let shallowest_end =
            self.column + Arrangement::BODY_INDENT + text.chars().count() + trailing;
        shallowest_end > Block::WIDTH
    }

    fn unit_trailing(&self, index: usize) -> usize {
        let is_last = index + 1 == self.plan.units.len();
        if is_last && self.list.closing_comments.is_empty() {
            self.trailing + Self::CLOSE.len()
        } else {
            0
        }
    }

    fn close<L: LaidOut>(&self, state: &mut AssemblyState<L>) {
        if self.list.closing_comments.is_empty() {
            state.laid_out.append(Self::CLOSE);
            return;
        }
        let indent = state.indent(self.column);
        for comment in &self.list.closing_comments {
            if comment.blank_before {
                state.laid_out.push_blank();
            }
            state
                .laid_out
                .push_indented(indent, L::text(&comment.text, indent));
        }
        state
            .laid_out
            .push_indented(indent, L::text(Self::CLOSE, indent));
    }
}

impl<L> AssemblyState<L> {
    fn indent(&self, column: usize) -> usize {
        self.later_indent
            .unwrap_or(column + Arrangement::BODY_INDENT)
    }
}

#[cfg(test)]
mod tests {
    use super::{Block, LayoutEngine};
    use crate::format::shapes::ShapeCatalog;
    use crate::format::syntax_tree::SyntaxTree;

    struct ChainFixture;

    impl ChainFixture {
        const DEPTH: usize = 400;
        const PLACEMENTS_PER_LIST: usize = 24;

        fn placements(depth: usize) -> usize {
            let text = format!("{}x{}", "(call ".repeat(depth), ")".repeat(depth));
            let tree = SyntaxTree::build(&text);
            let shapes = ShapeCatalog::default();
            let mut engine = LayoutEngine::new(&shapes);
            let _laid_out: Block = engine.node(&tree.entries[0].node, 0, 0);
            engine.placement_count()
        }
    }

    #[test]
    fn a_deep_chain_lays_out_each_list_at_a_bounded_number_of_placements() {
        let placements = ChainFixture::placements(ChainFixture::DEPTH);
        assert!(placements <= ChainFixture::DEPTH * ChainFixture::PLACEMENTS_PER_LIST);
    }
}
