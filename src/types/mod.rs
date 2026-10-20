mod checker;
mod copy;
mod display;
mod finalize;
mod graph;
mod infer;
mod literal_fit;
mod lowering;
mod problems;
mod structs;
mod traits;
mod ty;
mod typed;
mod unifier;

pub use checker::TypeChecker;
pub use copy::CopyRules;
pub use display::TypeDisplay;
pub use structs::{StructDefinition, StructTable};
pub use traits::{RustTrait, TraitTable};
pub use ty::{GenericId, GenericParam, Scheme, Type, TypeVar};
pub use typed::{
    Borrow, TypedArgument, TypedBinder, TypedBlock, TypedBodyItem, TypedDefinition, TypedExpr,
    TypedExprKind, TypedLambda, TypedModule, TypedProgram,
};
