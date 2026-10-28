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
struct MovedValue {
    site: MoveSite,
    paths: HashSet<FieldPath>,
}

#[derive(Clone, Default)]
pub struct MovedValues {
    values: HashMap<BindingId, MovedValue>,
}

impl MovedValues {
    pub fn site(&self, binding: BindingId) -> Option<MoveSite> {
        self.values.get(&binding).map(|moved| moved.site.clone())
    }

    pub fn move_whole(&mut self, binding: BindingId, span: Span) {
        let moved = MovedValue {
            site: MoveSite::Whole(span),
            paths: HashSet::new(),
        };
        self.values.insert(binding, moved);
    }

    pub fn move_field(&mut self, binding: BindingId, path: FieldPath, site: MoveSite) {
        self.values
            .entry(binding)
            .or_insert_with(|| MovedValue {
                site,
                paths: HashSet::new(),
            })
            .paths
            .insert(path);
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
            .filter(|(binding, _)| !earlier.values.contains_key(binding))
            .map(|(binding, moved)| (*binding, moved.clone()))
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
    fn refill(&mut self, place: &[Rc<str>], refilled: &HashSet<&str>) -> bool {
        if matches!(self.site, MoveSite::Whole(_)) {
            return false;
        }
        self.paths
            .retain(|path| !Self::is_refilled(path, place, refilled));
        self.paths.is_empty()
    }

    fn is_refilled(path: &[Rc<str>], place: &[Rc<str>], refilled: &HashSet<&str>) -> bool {
        path.starts_with(place)
            && path
                .get(place.len())
                .is_some_and(|field| refilled.contains(&**field))
    }

    fn absorb(&mut self, other: MovedValue) {
        if matches!(other.site, MoveSite::Whole(_)) {
            *self = other;
        } else if matches!(self.site, MoveSite::Field { .. }) {
            self.paths.extend(other.paths);
        }
    }
}
