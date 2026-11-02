#[derive(Default)]
pub struct StageWork {
    pub lowered_forms: Option<usize>,
    pub inferred_expressions: Option<usize>,
    pub emitted_expressions: Option<usize>,
    pub scanned_tokens: Option<usize>,
}
