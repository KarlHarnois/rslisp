use std::collections::HashSet;

use crate::core::BindingId;
use crate::types::{Lifetime, RustType, Type, TypedProgram};

pub struct OpaqueBorrows<'a> {
    program: &'a TypedProgram,
}

impl<'a> OpaqueBorrows<'a> {
    pub fn new(program: &'a TypedProgram) -> Self {
        Self { program }
    }

    pub fn holds_borrow(&self, ty: &Type) -> bool {
        match ty {
            Type::Named(binding, _) => {
                self.is_opaque(*binding) && self.program.type_holds_borrow(ty)
            }
            _ => false,
        }
    }

    pub fn holds_elided_borrow(&self, ty: &Type) -> bool {
        match ty {
            Type::Ref(Lifetime::Elided, _) | Type::Mut(Lifetime::Elided, _) => true,
            Type::Named(binding, _) if self.program.has_lifetime_parameter(*binding) => true,
            Type::Function(..) => false,
            _ => Self::parts(ty)
                .into_iter()
                .any(|part| self.holds_elided_borrow(part)),
        }
    }

    pub fn written_lifetimes(&self, ty: &Type) -> HashSet<BindingId> {
        let mut lifetimes = HashSet::new();
        self.collect_opaque(ty, &mut lifetimes);
        lifetimes
    }

    fn collect_opaque(&self, ty: &Type, lifetimes: &mut HashSet<BindingId>) {
        match ty {
            Type::Named(binding, arguments) if self.is_opaque(*binding) => {
                for argument in arguments {
                    Self::collect_written(argument, lifetimes);
                }
            }
            _ => {
                for part in Self::parts(ty) {
                    self.collect_opaque(part, lifetimes);
                }
            }
        }
    }

    fn collect_written(ty: &Type, lifetimes: &mut HashSet<BindingId>) {
        if let Some(Lifetime::Written(binding)) = ty.reference_lifetime() {
            lifetimes.insert(binding);
        }
        for part in Self::parts(ty) {
            Self::collect_written(part, lifetimes);
        }
    }

    fn is_opaque(&self, binding: BindingId) -> bool {
        self.program
            .rust_types
            .get(binding)
            .is_some_and(RustType::is_opaque)
    }

    fn parts(ty: &Type) -> Vec<&Type> {
        match ty {
            Type::Ref(_, inner) | Type::Mut(_, inner) => vec![inner.as_ref()],
            Type::Named(_, arguments) => arguments.iter().collect(),
            Type::Function(_, parameters, result) => parameters
                .iter()
                .chain(std::iter::once(result.as_ref()))
                .collect(),
            Type::Var(_) | Type::Generic(_) | Type::Primitive(_) | Type::TraitObject(_) => {
                Vec::new()
            }
        }
    }
}
