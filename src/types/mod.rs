mod associated;
mod binding;
mod boxes;
mod checker;
mod collection;
mod copy;
mod coverage;
mod derives;
mod display;
mod enums;
mod finalize;
mod graph;
mod infer;
mod literal_fit;
mod lowering;
mod pending;
mod pinning;
mod problems;
mod projection;
mod rust_types;
mod selection;
mod size;
mod string_view;
mod structs;
mod templates;
mod traits;
mod ty;
mod typed;
mod unifier;
mod walk;
mod wrapper;

pub use crate::core::ClosureKind;
pub use binding::GenericBinding;
pub use checker::TypeChecker;
pub use collection::{Collection, CollectionKind};
pub use copy::{CopyRules, InferredCopies};
pub use display::TypeDisplay;
pub use enums::{EnumDefinition, EnumTable, VariantDefinition, VariantPayload};
pub use graph::DefinitionGraph;
pub use pinning::{InstantiationReport, LiteralDefaults, Pinning};
pub use projection::{AssociatedType, ProjectionTable};
pub use rust_types::{RustType, RustTypeTable};
pub use structs::{StructDefinition, StructTable};
pub use templates::TemplateBorrows;
pub use traits::{Implementor, RustTrait, TraitTable};
pub use ty::{Bound, GenericId, GenericParam, Lifetime, Scheme, Type, TypeVar};
pub use typed::{
    Borrow, FieldPlace, PendingModeCheck, ReceiverKind, TraitMethodInfo, TypedArgument, TypedArm,
    TypedAssociatedType, TypedBinder, TypedBlock, TypedBodyItem, TypedControl, TypedConversion,
    TypedDefinition, TypedExpr, TypedExprKind, TypedLambda, TypedLeaf, TypedMatch, TypedModule,
    TypedPattern, TypedPatternKind, TypedProgram, TypedStructExpr, TypedTrait, TypedTraitImpl,
    TypedTraitMethod,
};
pub use wrapper::{FunctionValueWrapper, PreparedFunctionValue};
