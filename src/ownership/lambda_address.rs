use std::ptr;

use crate::types::TypedLambda;

// An address names one lambda only while that lambda stays alive: drop every
// address before a lambda it names can be freed, or a new lambda can take it.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct LambdaAddress(*const TypedLambda);

impl LambdaAddress {
    pub fn of(lambda: &TypedLambda) -> Self {
        Self(ptr::from_ref(lambda))
    }
}
