#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagnosticCode(&'static str);

impl DiagnosticCode {
    pub const UNCLOSED_LIST: Self = Self("L0101");
    pub const UNMATCHED_CLOSE: Self = Self("L0102");
    pub const UNTERMINATED_STRING: Self = Self("L0103");
    pub const UNKNOWN_ESCAPE: Self = Self("L0104");
    pub const MALFORMED_NUMBER: Self = Self("L0105");
    pub const RESERVED_CHARACTER: Self = Self("L0106");
    pub const NUMBER_TOO_LARGE: Self = Self("L0107");

    pub const UNBOUND_NAME: Self = Self("L0201");
    pub const AMBIGUOUS_NAME: Self = Self("L0202");
    pub const DUPLICATE_DEFINITION: Self = Self("L0203");
    pub const UNKNOWN_MODULE: Self = Self("L0204");
    pub const MISSING_EXPORT: Self = Self("L0205");
    pub const IMPORT_CYCLE: Self = Self("L0206");
    pub const NOT_A_DEFINITION: Self = Self("L0207");
    pub const MODULE_LEVEL_ONLY: Self = Self("L0208");
    pub const EMPTY_CALL: Self = Self("L0209");
    pub const MALFORMED_FORM: Self = Self("L0210");
    pub const FORM_AS_VALUE: Self = Self("L0211");
    pub const WRONG_NAMESPACE: Self = Self("L0212");

    pub const MACRO_FAILED: Self = Self("L0301");
    pub const MACRO_RESULT_NOT_FORM: Self = Self("L0302");
    pub const COMPILE_TIME_ERROR: Self = Self("L0303");
    pub const MACRO_AS_VALUE: Self = Self("L0304");
    pub const EXPANSION_TOO_DEEP: Self = Self("L0305");
    pub const NO_COMPILE_TIME_TWIN: Self = Self("L0306");
    pub const COMPILE_TIME_CYCLE: Self = Self("L0307");

    pub fn as_str(self) -> &'static str {
        self.0
    }
}
