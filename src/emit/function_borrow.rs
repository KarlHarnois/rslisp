use crate::types::ClosureKind;

#[derive(Clone, Copy)]
pub(super) enum FunctionBorrow {
    Shared,
    Mutable,
}

impl FunctionBorrow {
    pub(super) fn of(kind: ClosureKind) -> Self {
        match kind {
            ClosureKind::Fn => Self::Shared,
            ClosureKind::FnMut | ClosureKind::FnOnce => Self::Mutable,
        }
    }

    pub(super) fn prefix(self) -> &'static str {
        match self {
            Self::Shared => "&",
            Self::Mutable => "&mut ",
        }
    }

    pub(super) fn trait_object(self, binder: &str) -> String {
        let callable = match self {
            Self::Shared => ClosureKind::Fn,
            Self::Mutable => ClosureKind::FnMut,
        };
        format!("{}dyn {binder}{}", self.prefix(), callable.keyword())
    }
}
