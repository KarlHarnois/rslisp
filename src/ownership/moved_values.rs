use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::core::BindingId;
use crate::source::Span;

#[derive(Clone)]
pub enum MoveSite {
    Whole(Span),
    Field { span: Span, place: Rc<str> },
}

impl MoveSite {
    pub fn span(&self) -> Span {
        match self {
            Self::Whole(span) | Self::Field { span, .. } => *span,
        }
    }

    pub fn cloned_place<'a>(&'a self, name: &'a str) -> &'a str {
        match self {
            Self::Whole(_) => name,
            Self::Field { place, .. } => place,
        }
    }
}

pub type FieldPath = Vec<Rc<str>>;

#[derive(Clone)]
enum MovedValue {
    Whole(Span),
    Fields(Vec<MovedField>),
}

#[derive(Clone)]
struct MovedField {
    path: FieldPath,
    site: MoveSite,
}

#[derive(Clone, Default)]
pub struct MovedValues {
    values: HashMap<BindingId, MovedValue>,
}

impl MovedValues {
    pub fn site(&self, binding: BindingId) -> Option<MoveSite> {
        self.site_overlapping(binding, std::iter::empty())
    }

    pub fn site_overlapping<'p>(
        &self,
        binding: BindingId,
        read: impl Iterator<Item = &'p str> + Clone,
    ) -> Option<MoveSite> {
        self.values.get(&binding)?.site_overlapping(read)
    }

    pub fn move_whole(&mut self, binding: BindingId, span: Span) {
        self.values.insert(binding, MovedValue::Whole(span));
    }

    pub fn move_field(&mut self, binding: BindingId, path: FieldPath, site: MoveSite) {
        let moved = self
            .values
            .entry(binding)
            .or_insert_with(|| MovedValue::Fields(Vec::new()));
        moved.add_field(MovedField { path, site });
    }

    pub fn refill<'a>(
        &mut self,
        binding: BindingId,
        place: &[Rc<str>],
        fields: impl IntoIterator<Item = &'a str>,
    ) {
        let Some(moved) = self.values.get_mut(&binding) else {
            return;
        };
        let refilled: HashSet<&str> = fields.into_iter().collect();
        if moved.refill(place, &refilled) {
            self.values.remove(&binding);
        }
    }

    pub fn moved_since(&self, earlier: &MovedValues) -> MovedValues {
        let values = self
            .values
            .iter()
            .filter_map(|(binding, moved)| {
                let since = moved.moved_since(earlier.values.get(binding))?;
                Some((*binding, since))
            })
            .collect();
        MovedValues { values }
    }

    pub fn retain(&mut self, mut keep: impl FnMut(BindingId) -> bool) {
        self.values.retain(|binding, _| keep(*binding));
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn bindings(&self) -> impl Iterator<Item = BindingId> + '_ {
        self.values.keys().copied()
    }

    pub fn merge(&mut self, other: MovedValues) {
        for (binding, moved) in other.values {
            match self.values.get_mut(&binding) {
                Some(existing) => existing.absorb(moved),
                None => {
                    self.values.insert(binding, moved);
                }
            }
        }
    }
}

impl MovedValue {
    fn site_overlapping<'p>(
        &self,
        read: impl Iterator<Item = &'p str> + Clone,
    ) -> Option<MoveSite> {
        match self {
            Self::Whole(span) => Some(MoveSite::Whole(*span)),
            Self::Fields(fields) => fields
                .iter()
                .find(|field| field.overlaps(read.clone()))
                .map(|field| field.site.clone()),
        }
    }

    fn add_field(&mut self, field: MovedField) {
        if let Self::Fields(fields) = self
            && !fields.iter().any(|moved| moved.path == field.path)
        {
            fields.push(field);
        }
    }

    fn refill(&mut self, place: &[Rc<str>], refilled: &HashSet<&str>) -> bool {
        let Self::Fields(fields) = self else {
            return false;
        };
        fields.retain(|field| !field.is_refilled(place, refilled));
        fields.is_empty()
    }

    fn moved_since(&self, earlier: Option<&MovedValue>) -> Option<MovedValue> {
        match (self, earlier) {
            (_, None) | (Self::Whole(_), Some(Self::Fields(_))) => Some(self.clone()),
            (_, Some(Self::Whole(_))) => None,
            (Self::Fields(fields), Some(Self::Fields(earlier))) => {
                Self::fields_moved_since(fields, earlier)
            }
        }
    }

    fn fields_moved_since(fields: &[MovedField], earlier: &[MovedField]) -> Option<MovedValue> {
        let since: Vec<MovedField> = fields
            .iter()
            .filter(|field| !earlier.iter().any(|moved| moved.path == field.path))
            .cloned()
            .collect();
        (!since.is_empty()).then_some(Self::Fields(since))
    }

    fn absorb(&mut self, other: MovedValue) {
        match other {
            Self::Whole(_) => *self = other,
            Self::Fields(fields) => fields.into_iter().for_each(|field| self.add_field(field)),
        }
    }
}

impl MovedField {
    fn overlaps<'p>(&self, read: impl Iterator<Item = &'p str>) -> bool {
        self.path
            .iter()
            .zip(read)
            .all(|(moved, read)| **moved == *read)
    }

    fn is_refilled(&self, place: &[Rc<str>], refilled: &HashSet<&str>) -> bool {
        self.path.starts_with(place)
            && self
                .path
                .get(place.len())
                .is_some_and(|field| refilled.contains(&**field))
    }
}
