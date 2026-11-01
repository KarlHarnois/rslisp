mod callee_parameters;
mod expressions;
mod fragment;
mod fresh_lifetimes;
mod function_borrow;
mod generic_positions;
mod items;
mod let_annotation;
mod literal_generics;
mod local_closures;
mod locals;
pub(crate) mod mangle;
mod method_elisions;
mod mutable_locals;
mod opaque_borrows;
mod open_literals;
mod operator;
mod plan;
mod problems;
mod result_borrows;
mod runtime;
mod rust_fallback;
mod signature_borrows;
mod store_sites;
mod stored_arguments;
mod stored_functions;
mod template;
mod type_anchor;
mod types;
mod unsized_parameters;
mod untied_borrows;
mod writer;

pub use writer::SpanMap;

use crate::diagnostic::Diagnostics;
use crate::source::Span;
use crate::types::TypedProgram;
use items::ItemEmitter;
use plan::EmitPlan;
use unsized_parameters::UnsizedParameters;
use writer::RustWriter;

pub struct EmittedCrate {
    pub code: String,
    pub span_map: SpanMap,
    pub names: Vec<(String, String)>,
}

pub struct RustEmitter;

impl RustEmitter {
    pub fn emit(program: &TypedProgram, entry: Span) -> Result<EmittedCrate, Diagnostics> {
        let plan = EmitPlan::build(program, entry)?;
        let unsized_parameters = UnsizedParameters::of(program, &plan);
        let mut items = ItemEmitter::new(program, &plan, &unsized_parameters);
        let fragments = items.crate_fragments();
        let mut problems = Diagnostics::default();
        items
            .problems
            .into_iter()
            .for_each(|problem| problems.push(problem));
        let mut writer = RustWriter::default();
        writer.write(&fragments);
        let (code, span_map) = writer.finish();
        let mut names = items.names;
        names.extend(
            plan.rust_names()
                .map(|(rust, user)| (rust.to_string(), user.to_string())),
        );
        problems.into_result(EmittedCrate {
            code,
            span_map,
            names,
        })
    }
}
