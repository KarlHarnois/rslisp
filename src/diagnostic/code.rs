use std::borrow::Cow;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticCode(Cow<'static, str>);

impl DiagnosticCode {
    pub const UNCLOSED_LIST: Self = Self(Cow::Borrowed("L0101"));
    pub const UNMATCHED_CLOSE: Self = Self(Cow::Borrowed("L0102"));
    pub const UNTERMINATED_STRING: Self = Self(Cow::Borrowed("L0103"));
    pub const UNKNOWN_ESCAPE: Self = Self(Cow::Borrowed("L0104"));
    pub const MALFORMED_NUMBER: Self = Self(Cow::Borrowed("L0105"));
    pub const RESERVED_CHARACTER: Self = Self(Cow::Borrowed("L0106"));
    pub const NUMBER_TOO_LARGE: Self = Self(Cow::Borrowed("L0107"));

    pub const UNBOUND_NAME: Self = Self(Cow::Borrowed("L0201"));
    pub const AMBIGUOUS_NAME: Self = Self(Cow::Borrowed("L0202"));
    pub const DUPLICATE_DEFINITION: Self = Self(Cow::Borrowed("L0203"));
    pub const UNKNOWN_MODULE: Self = Self(Cow::Borrowed("L0204"));
    pub const MISSING_EXPORT: Self = Self(Cow::Borrowed("L0205"));
    pub const IMPORT_CYCLE: Self = Self(Cow::Borrowed("L0206"));
    pub const NOT_A_DEFINITION: Self = Self(Cow::Borrowed("L0207"));
    pub const MODULE_LEVEL_ONLY: Self = Self(Cow::Borrowed("L0208"));
    pub const EMPTY_CALL: Self = Self(Cow::Borrowed("L0209"));
    pub const MALFORMED_FORM: Self = Self(Cow::Borrowed("L0210"));
    pub const FORM_AS_VALUE: Self = Self(Cow::Borrowed("L0211"));
    pub const WRONG_NAMESPACE: Self = Self(Cow::Borrowed("L0212"));

    pub const MACRO_FAILED: Self = Self(Cow::Borrowed("L0301"));
    pub const MACRO_RESULT_NOT_FORM: Self = Self(Cow::Borrowed("L0302"));
    pub const COMPILE_TIME_ERROR: Self = Self(Cow::Borrowed("L0303"));
    pub const MACRO_AS_VALUE: Self = Self(Cow::Borrowed("L0304"));
    pub const EXPANSION_TOO_DEEP: Self = Self(Cow::Borrowed("L0305"));
    pub const PROGRAM_ONLY: Self = Self(Cow::Borrowed("L0306"));
    pub const COMPILE_TIME_CYCLE: Self = Self(Cow::Borrowed("L0307"));

    pub const MISMATCHED_TYPES: Self = Self(Cow::Borrowed("L0401"));
    pub const WRONG_ARGUMENT_COUNT: Self = Self(Cow::Borrowed("L0402"));
    pub const NOT_A_FUNCTION: Self = Self(Cow::Borrowed("L0403"));
    pub const MISSING_TRAIT: Self = Self(Cow::Borrowed("L0404"));
    pub const LITERAL_OUT_OF_RANGE: Self = Self(Cow::Borrowed("L0406"));
    pub const CANNOT_INFER: Self = Self(Cow::Borrowed("L0407"));
    pub const UNKNOWN_FIELD: Self = Self(Cow::Borrowed("L0408"));
    pub const MISSING_FIELDS: Self = Self(Cow::Borrowed("L0409"));
    pub const NO_FIELDS: Self = Self(Cow::Borrowed("L0410"));
    pub const INFINITE_TYPE: Self = Self(Cow::Borrowed("L0411"));
    pub const COMPILE_TIME_ONLY: Self = Self(Cow::Borrowed("L0412"));
    pub const MISPLACED_WHERE: Self = Self(Cow::Borrowed("L0413"));
    pub const LITERAL_TYPE_PARAMETER: Self = Self(Cow::Borrowed("L0414"));
    pub const WRONG_TYPE_ARGUMENT_COUNT: Self = Self(Cow::Borrowed("L0415"));
    pub const UNUSED_TYPE_PARAMETER: Self = Self(Cow::Borrowed("L0416"));
    pub const BOUNDED_TYPE_PARAMETER: Self = Self(Cow::Borrowed("L0417"));

    pub const USE_AFTER_MOVE: Self = Self(Cow::Borrowed("L0501"));
    pub const AMBIGUOUS_BORROW: Self = Self(Cow::Borrowed("L0502"));
    pub const NEEDS_OWNERSHIP: Self = Self(Cow::Borrowed("L0503"));

    pub const MISSING_MAIN: Self = Self(Cow::Borrowed("L0601"));
    pub const NAME_COLLISION: Self = Self(Cow::Borrowed("L0602"));
    pub const BAD_TEMPLATE: Self = Self(Cow::Borrowed("L0603"));
    pub const RUST_ERROR: Self = Self(Cow::Borrowed("L0604"));

    pub fn from_rustc(code: String) -> Self {
        Self(Cow::Owned(code))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
