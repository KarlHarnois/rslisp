use std::rc::Rc;

use super::{Builtin, KernelForm, KernelType, ModuleId};
use crate::source::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BindingId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Namespace {
    Value,
    Type,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingKind {
    KernelForm(KernelForm),
    KernelType(KernelType),
    Boolean(bool),
    CodeBuiltin(Builtin),
    Macro,
    Global(ModuleId),
    Local,
    Struct(ModuleId),
    Trait(ModuleId),
    TypeVariable,
}

#[derive(Clone, Debug)]
pub struct BindingInfo {
    name: Rc<str>,
    kind: BindingKind,
    span: Option<Span>,
}

impl BindingInfo {
    pub fn new(name: Rc<str>, kind: BindingKind, span: Option<Span>) -> Self {
        Self { name, kind, span }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> BindingKind {
        self.kind
    }

    pub fn span(&self) -> Option<Span> {
        self.span
    }
}

#[derive(Default)]
pub struct BindingArena {
    infos: Vec<BindingInfo>,
}

impl BindingArena {
    pub fn add(&mut self, info: BindingInfo) -> BindingId {
        let id = BindingId(u32::try_from(self.infos.len()).expect("fewer than 2^32 bindings"));
        self.infos.push(info);
        id
    }

    pub fn get(&self, id: BindingId) -> &BindingInfo {
        &self.infos[id.0 as usize]
    }
}
