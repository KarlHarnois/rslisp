use std::collections::{HashMap, HashSet};

use super::mangle::NameMangler;
use crate::core::BindingId;

pub struct LocalNames {
    used: HashSet<String>,
    assigned: HashMap<BindingId, String>,
    scopes: Vec<Vec<String>>,
}

impl LocalNames {
    const FIRST_SUFFIX: usize = 2;

    pub fn new(reserved: impl IntoIterator<Item = String>) -> Self {
        Self {
            used: reserved.into_iter().collect(),
            assigned: HashMap::new(),
            scopes: Vec::new(),
        }
    }

    pub fn enter(&mut self) {
        self.scopes.push(Vec::new());
    }

    pub fn leave(&mut self) {
        for name in self.scopes.pop().unwrap_or_default() {
            self.used.remove(&name);
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
        if let Some(scope) = self.scopes.last_mut() {
            scope.push(candidate.clone());
        }
        self.assigned.insert(binding, candidate.clone());
        candidate
    }

    pub fn bind_exact(&mut self, binding: BindingId, name: &str) {
        self.used.insert(name.to_string());
        if let Some(scope) = self.scopes.last_mut() {
            scope.push(name.to_string());
        }
        self.assigned.insert(binding, name.to_string());
    }

    pub fn bind_receiver(&mut self, binding: BindingId) -> String {
        let name = "self".to_string();
        self.used.insert(name.clone());
        if let Some(scope) = self.scopes.last_mut() {
            scope.push(name.clone());
        }
        self.assigned.insert(binding, name.clone());
        name
    }

    pub fn temporary(&mut self, base: &str) -> String {
        let mut candidate = base.to_string();
        let mut suffix = Self::FIRST_SUFFIX;
        while self.used.contains(&candidate) {
            candidate = format!("{base}_{suffix}");
            suffix += 1;
        }
        self.used.insert(candidate.clone());
        if let Some(scope) = self.scopes.last_mut() {
            scope.push(candidate.clone());
        }
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
