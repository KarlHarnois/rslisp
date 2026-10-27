mod expressions;
mod fragment;
mod items;
mod literal_generics;
mod locals;
pub(crate) mod mangle;
mod mutable_locals;
mod operator;
mod plan;
mod problems;
mod runtime;
mod signature_borrows;
mod template;
mod types;
mod unsized_parameters;
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
