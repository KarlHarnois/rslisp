use super::block::Block;
use super::shapes::{Shape, ShapeCatalog};
use super::syntax_tree::{Entry, ListNode};
use crate::core::{DefinitionForm, KernelForm, Keyword};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Single(usize),
    Pair(usize, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaterIndent {
    Past(usize),
    UnderLastOnFirstLine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arrangement {
    pub first_line_units: usize,
    pub later_indent: LaterIndent,
}

pub struct ListPlan {
    pub units: Vec<Unit>,
    pub arrangements: Vec<Arrangement>,
    pub docstring_entry: Option<usize>,
    pub fillable_units: Vec<bool>,
    pub binding_list_entry: Option<usize>,
    one_line_allowed: bool,
    first_arm_entry: Option<usize>,
}

impl Arrangement {
    pub const BODY_INDENT: usize = 2;
    const UNDER_HEAD_INDENT: usize = 1;

    fn body() -> LaterIndent {
        LaterIndent::Past(Self::BODY_INDENT)
    }

    fn aligned_after(first_line_units: usize) -> Self {
        Self {
            first_line_units,
            later_indent: LaterIndent::UnderLastOnFirstLine,
        }
    }
}

impl ListPlan {
    pub fn new(list: &ListNode, shapes: &ShapeCatalog) -> Self {
        let head = list
            .entries
            .first()
            .and_then(|entry| entry.node.symbol_name());
        let shape = head.and_then(|name| shapes.of(name));
        let header_length = shape.map_or(0, |shape| shape.header_length);
        let units = UnitGrouping::new(&list.entries, header_length).units();
        let docstring_entry = Self::docstring_entry(list, head, shapes);
        let fills_atom_runs = !shape.is_some_and(|shape| shape.one_per_line);
        let body_forms = list.entries.len().saturating_sub(header_length + 1);
        let definer = shape.is_some_and(|shape| shape.takes_docstring);
        Self {
            fillable_units: units
                .iter()
                .map(|unit| fills_atom_runs && Self::is_fillable(list, *unit, docstring_entry))
                .collect(),
            units,
            arrangements: Self::arrangements(head, shape),
            docstring_entry,
            binding_list_entry: Self::binding_list_entry(shape),
            one_line_allowed: !definer || body_forms <= 1,
            first_arm_entry: shape.filter(|shape| shape.arms).map(|_| header_length + 1),
        }
    }

    pub fn binding_list(list: &ListNode) -> Self {
        let units = BindingPairs::new(list.entries.len()).units();
        Self {
            fillable_units: vec![false; units.len()],
            units,
            arrangements: vec![Arrangement {
                first_line_units: 1,
                later_indent: LaterIndent::Past(Arrangement::UNDER_HEAD_INDENT),
            }],
            docstring_entry: None,
            binding_list_entry: None,
            one_line_allowed: list.entries.len() <= BindingPairs::PAIR_LENGTH,
            first_arm_entry: None,
        }
    }

    fn binding_list_entry(shape: Option<Shape>) -> Option<usize> {
        shape
            .filter(|shape| shape.bindings)
            .map(|shape| shape.header_length)
    }

    pub fn is_arm(&self, entry: usize) -> bool {
        self.first_arm_entry.is_some_and(|first| entry >= first)
    }

    pub fn allows_one_line(&self) -> bool {
        self.one_line_allowed
    }

    pub fn arrangements_at(&self, column: usize) -> &[Arrangement] {
        if column < Block::WIDTH {
            &self.arrangements
        } else {
            &self.arrangements[self.arrangements.len() - 1..]
        }
    }

    fn arrangements(head: Option<&str>, shape: Option<Shape>) -> Vec<Arrangement> {
        match (head, shape) {
            (None, _) => vec![Arrangement {
                first_line_units: 1,
                later_indent: LaterIndent::Past(Arrangement::UNDER_HEAD_INDENT),
            }],
            (Some(name), Some(shape)) if Self::is_kernel(name, DefinitionForm::Use) => {
                vec![Arrangement::aligned_after(shape.header_length + 2)]
            }
            (Some(name), None) if Self::is_kernel(name, DefinitionForm::Export) => {
                vec![Arrangement::aligned_after(2)]
            }
            (Some(_), Some(shape)) => vec![Arrangement {
                first_line_units: shape.header_length + 1,
                later_indent: Arrangement::body(),
            }],
            (Some(_), None) => vec![
                Arrangement::aligned_after(2),
                Arrangement {
                    first_line_units: 1,
                    later_indent: Arrangement::body(),
                },
            ],
        }
    }

    fn is_kernel(name: &str, form: DefinitionForm) -> bool {
        name == KernelForm::Definition(form).name()
    }

    fn is_fillable(list: &ListNode, unit: Unit, docstring_entry: Option<usize>) -> bool {
        let Unit::Single(entry) = unit else {
            return false;
        };
        let single_line_atom = list.entries[entry]
            .node
            .as_atom()
            .is_some_and(|atom| !atom.is_multi_line());
        single_line_atom && docstring_entry != Some(entry)
    }

    fn docstring_entry(
        list: &ListNode,
        head: Option<&str>,
        shapes: &ShapeCatalog,
    ) -> Option<usize> {
        let position = shapes.docstring_position(head?)?;
        let is_string = list.entries.get(position)?.node.as_atom()?.is_string();
        let followed = list.entries.len() > position + 1;
        (is_string && followed).then_some(position)
    }
}

struct UnitGrouping<'e> {
    entries: &'e [Entry],
    single_count: usize,
}

impl<'e> UnitGrouping<'e> {
    fn new(entries: &'e [Entry], header_length: usize) -> Self {
        Self {
            entries,
            single_count: (header_length + 1).min(entries.len()),
        }
    }

    fn units(&self) -> Vec<Unit> {
        let mut units: Vec<Unit> = (0..self.single_count).map(Unit::Single).collect();
        let mut index = self.single_count;
        while index < self.entries.len() {
            let unit = self.unit_at(index);
            index += unit.entry_count();
            units.push(unit);
        }
        units
    }

    fn unit_at(&self, index: usize) -> Unit {
        if self.starts_pair(index) {
            Unit::Pair(index, index + 1)
        } else {
            Unit::Single(index)
        }
    }

    fn starts_pair(&self, index: usize) -> bool {
        let entry = &self.entries[index];
        let is_keyword = entry
            .node
            .symbol_name()
            .is_some_and(|name| Keyword::field_name(name).is_some());
        let value_follows = self
            .entries
            .get(index + 1)
            .is_some_and(|value| !value.is_interrupted());
        is_keyword && entry.trailing_comment.is_none() && value_follows
    }
}

struct BindingPairs {
    entry_count: usize,
}

impl BindingPairs {
    const PAIR_LENGTH: usize = 2;

    fn new(entry_count: usize) -> Self {
        Self { entry_count }
    }

    fn units(&self) -> Vec<Unit> {
        (0..self.entry_count)
            .step_by(Self::PAIR_LENGTH)
            .map(|name| self.unit_at(name))
            .collect()
    }

    fn unit_at(&self, name: usize) -> Unit {
        let value = name + 1;
        if value < self.entry_count {
            Unit::Pair(name, value)
        } else {
            Unit::Single(name)
        }
    }
}

impl Unit {
    pub fn first_entry(self) -> usize {
        match self {
            Unit::Single(entry) | Unit::Pair(entry, _) => entry,
        }
    }

    pub fn last_entry(self) -> usize {
        match self {
            Unit::Single(entry) | Unit::Pair(_, entry) => entry,
        }
    }

    pub fn entries(self) -> impl Iterator<Item = usize> {
        self.first_entry()..=self.last_entry()
    }

    fn entry_count(self) -> usize {
        self.last_entry() - self.first_entry() + 1
    }
}
