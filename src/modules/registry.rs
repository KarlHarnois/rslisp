use std::collections::HashMap;
use std::rc::Rc;

use crate::core::{BindingId, ModuleId, ModulePath, Namespace};
use crate::scope::Scope;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoadStatus {
    Loading,
    Loaded,
}

pub struct ModuleRecord {
    path: ModulePath,
    base_scope: Scope,
    exports: HashMap<(Rc<str>, Namespace), BindingId>,
    status: LoadStatus,
}

impl ModuleRecord {
    pub fn path(&self) -> &ModulePath {
        &self.path
    }

    pub fn base_scope(&self) -> Scope {
        self.base_scope
    }

    pub fn export(&mut self, name: Rc<str>, namespace: Namespace, binding: BindingId) {
        self.exports.insert((name, namespace), binding);
    }

    pub fn exported(&self, name: &str, namespace: Namespace) -> Option<BindingId> {
        self.exports.get(&(Rc::from(name), namespace)).copied()
    }

    pub fn exports(&self) -> impl Iterator<Item = (&Rc<str>, Namespace, BindingId)> {
        self.exports
            .iter()
            .map(|((name, namespace), binding)| (name, *namespace, *binding))
    }

    pub fn is_loading(&self) -> bool {
        self.status == LoadStatus::Loading
    }
}

#[derive(Default)]
pub struct ModuleRegistry {
    records: Vec<ModuleRecord>,
    by_path: HashMap<ModulePath, ModuleId>,
}

impl ModuleRegistry {
    pub fn register(&mut self, path: ModulePath, base_scope: Scope) -> ModuleId {
        let id = ModuleId::new(self.records.len());
        self.by_path.insert(path.clone(), id);
        self.records.push(ModuleRecord {
            path,
            base_scope,
            exports: HashMap::new(),
            status: LoadStatus::Loading,
        });
        id
    }

    pub fn find(&self, path: &ModulePath) -> Option<ModuleId> {
        self.by_path.get(path).copied()
    }

    pub fn finish_loading(&mut self, id: ModuleId) {
        self.records[id.index()].status = LoadStatus::Loaded;
    }

    pub fn loading_paths(&self) -> Vec<&ModulePath> {
        self.records
            .iter()
            .filter(|record| record.is_loading())
            .map(ModuleRecord::path)
            .collect()
    }

    pub fn get(&self, id: ModuleId) -> &ModuleRecord {
        &self.records[id.index()]
    }

    pub fn get_mut(&mut self, id: ModuleId) -> &mut ModuleRecord {
        &mut self.records[id.index()]
    }
}
