mod checker;
mod copy;
mod display;
mod finalize;
mod graph;
mod infer;
mod literal_fit;
mod lowering;
mod problems;
mod rust_types;
mod selection;
mod structs;
mod traits;
mod ty;
mod typed;
mod unifier;
mod walk;

pub use checker::TypeChecker;
pub use copy::CopyRules;
pub use display::TypeDisplay;
pub use rust_types::{RustType, RustTypeTable};
pub use structs::{StructDefinition, StructTable};
pub use traits::{RustTrait, TraitTable};
pub use ty::{GenericId, GenericParam, Scheme, Type, TypeVar};
pub use typed::{
    Borrow, TypedArgument, TypedBinder, TypedBlock, TypedBodyItem, TypedControl, TypedDefinition,
    TypedExpr, TypedExprKind, TypedLambda, TypedLeaf, TypedModule, TypedProgram, TypedStructExpr,
};
