use std::collections::{HashMap, HashSet};

use super::mangle::NameMangler;
use crate::core::BindingId;

pub struct LocalNames {
    used: HashSet<String>,
    assigned: HashMap<BindingId, String>,
}

impl LocalNames {
    const FIRST_SUFFIX: usize = 2;

    pub fn new(reserved: impl IntoIterator<Item = String>) -> Self {
        Self {
            used: reserved.into_iter().collect(),
            assigned: HashMap::new(),
        }
    }

    pub fn bind(&mut self, binding: BindingId, name: &str) -> String {
        let base = NameMangler::value(name);
        let mut candidate = base.clone();
        let mut suffix = Self::FIRST_SUFFIX;
        while self.used.contains(&candidate) {
            candidate = format!("{base}_{suffix}");
            suffix += 1;
        }
        self.used.insert(candidate.clone());
        self.assigned.insert(binding, candidate.clone());
        candidate
    }

    pub fn get(&self, binding: BindingId) -> Option<&str> {
        self.assigned.get(&binding).map(String::as_str)
    }

    pub fn pairs(&self) -> impl Iterator<Item = (BindingId, &str)> {
        self.assigned
            .iter()
            .map(|(binding, name)| (*binding, name.as_str()))
    }
}
