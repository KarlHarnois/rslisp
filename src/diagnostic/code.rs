use std::borrow::Cow;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DiagnosticCode(Cow<'static, str>);

macro_rules! diagnostic_codes {
    ($($constant:ident = $code:literal,)*) => {
        impl DiagnosticCode {
            $(pub const $constant: Self = Self(Cow::Borrowed($code));)*

            const NAMED: &[(&str, &str)] = &[$((stringify!($constant), $code)),*];
        }
    };
}

diagnostic_codes! {
    UNCLOSED_LIST = "L0101",
    UNMATCHED_CLOSE = "L0102",
    UNTERMINATED_STRING = "L0103",
    UNKNOWN_ESCAPE = "L0104",
    MALFORMED_NUMBER = "L0105",
    RESERVED_CHARACTER = "L0106",
    NUMBER_TOO_LARGE = "L0107",
    LIST_TOO_DEEP = "L0108",

    UNBOUND_NAME = "L0201",
    AMBIGUOUS_NAME = "L0202",
    DUPLICATE_DEFINITION = "L0203",
    UNKNOWN_MODULE = "L0204",
    MISSING_EXPORT = "L0205",
    IMPORT_CYCLE = "L0206",
    NOT_A_DEFINITION = "L0207",
    MODULE_LEVEL_ONLY = "L0208",
    EMPTY_CALL = "L0209",
    MALFORMED_FORM = "L0210",
    FORM_AS_VALUE = "L0211",
    WRONG_NAMESPACE = "L0212",
    BAD_TYPE_TEMPLATE = "L0213",
    RECUR_OUTSIDE_TAIL = "L0214",
    RECUR_ARGUMENT_COUNT = "L0215",
    RECUR_OUTSIDE_FN = "L0216",
    SECOND_TRAIT_PATH = "L0217",
    MALFORMED_TEMPLATE = "L0218",
    TYPE_PARAMETERS_ON_NAME = "L0219",
    TRY_OUTSIDE_FN = "L0220",
    TYPE_PARAMETER_PLACEHOLDER = "L0221",
    TYPE_PLACEHOLDER_IN_RUST_TYPE = "L0222",
    RUST_TYPE_CLAUSES_NEED_ONE_PARAMETER = "L0223",
    BORROWED_TYPE_PLACEHOLDER = "L0224",
    PLACEHOLDER_WITHOUT_PARAMETER = "L0225",
    UNUSED_TYPE_PLACEHOLDER = "L0226",
    LET_OUTSIDE_BODY = "L0227",
    MALFORMED_TYPE = "L0228",
    MACRO_OUTSIDE_DEF = "L0229",
    MIXED_VARIANT_PAYLOAD = "L0230",
    NOT_A_PATTERN = "L0231",
    ARM_BODY = "L0232",
    VARIANT_AS_VALUE = "L0233",
    NOT_A_TYPE = "L0234",
    NOT_A_NAME = "L0235",
    SIGNATURE_TARGET = "L0236",
    DERIVE_TARGET = "L0237",
    DERIVE_TRAIT = "L0238",
    MISSING_METHOD = "L0239",
    EXTRA_METHOD = "L0240",
    UNDECLARED_METHOD = "L0241",
    GENERIC_IMPLEMENTOR = "L0242",
    FIELD_KEYWORD_ARITY = "L0243",
    KEYWORD_BINDER = "L0244",
    IGNORED_BINDING_READ = "L0245",

    MACRO_FAILED = "L0301",
    MACRO_RESULT_NOT_FORM = "L0302",
    COMPILE_TIME_ERROR = "L0303",
    MACRO_AS_VALUE = "L0304",
    EXPANSION_TOO_DEEP = "L0305",
    PROGRAM_ONLY = "L0306",
    COMPILE_TIME_CYCLE = "L0307",
    BROKEN_DEFINITION = "L0308",
    BROKEN_MACRO = "L0309",
    NOT_AT_COMPILE_TIME = "L0310",
    COMPILE_TIME_ARGUMENT_COUNT = "L0311",
    NOT_CALLABLE = "L0312",
    NOT_A_BOOLEAN = "L0313",
    INDEX_OUT_OF_RANGE = "L0314",
    ARITHMETIC_OVERFLOW = "L0315",
    RUNTIME_ONLY = "L0316",
    MATCH_AT_RUNTIME = "L0317",
    COMPILE_TIME_RECURSION = "L0318",
    RUST_CODE_PROGRAM_ONLY = "L0319",
    REPEATED_PARAMETER = "L0320",
    FAILURE_PART = "L0321",
    UNKNOWN_FAILURE_CODE = "L0322",
    UNKNOWN_PARAMETER = "L0323",
    UNKNOWN_FIELD_DEFAULT = "L0324",
    EXPANSION_NESTS_TOO_DEEP = "L0325",
    UNDOCUMENTED_TERM = "L0327",
    DOCSTRING_BEFORE_PARAMETERS = "L0328",

    MISMATCHED_TYPES = "L0401",
    WRONG_ARGUMENT_COUNT = "L0402",
    NOT_A_FUNCTION = "L0403",
    MISSING_TRAIT = "L0404",
    LITERAL_OUT_OF_RANGE = "L0406",
    CANNOT_INFER = "L0407",
    UNKNOWN_FIELD = "L0408",
    MISSING_FIELDS = "L0409",
    NO_FIELDS = "L0410",
    INFINITE_TYPE = "L0411",
    COMPILE_TIME_ONLY = "L0412",
    MISPLACED_WHERE = "L0413",
    LITERAL_TYPE_PARAMETER = "L0414",
    WRONG_TYPE_ARGUMENT_COUNT = "L0415",
    UNUSED_TYPE_PARAMETER = "L0416",
    BOUNDED_TYPE_PARAMETER = "L0417",
    BARE_IMPLEMENTOR = "L0418",
    MUT_BORROW_OF_COPY = "L0419",
    TRAIT_CLAIM_ON_STRUCT = "L0421",
    NOT_DERIVABLE = "L0422",
    BLOCKED_DERIVE = "L0423",
    DUPLICATE_DERIVE = "L0424",
    TRAIT_NO_SECOND_TYPE = "L0425",
    MISPLACED_WITH = "L0426",
    TRY_TARGET = "L0428",
    TRY_NOT_RESULT_OR_OPTION = "L0429",
    TRY_NO_CONVERSION = "L0437",
    AMBIGUOUS_VARIANT = "L0430",
    UNKNOWN_VARIANT = "L0431",
    WRONG_PATTERN_COUNT = "L0432",
    NON_EXHAUSTIVE_MATCH = "L0433",
    UNREACHABLE_ARM = "L0434",
    DERIVE_ON_RUST_TYPE = "L0435",
    VARIANT_ARM_VALUE = "L0436",
    ENUM_RUST_TEMPLATE = "L0438",
    RECURSIVELY_USED_TYPE_PARAMETER = "L0439",
    MISSING_BOUND = "L0440",
    LITERAL_MISFIT = "L0441",
    QUOTE_AT_RUNTIME = "L0442",
    UNUSED_ENUM_PARAMETER = "L0443",
    UNSIZABLE_IMPLEMENTOR = "L0444",
    FOREIGN_IMPLEMENTOR = "L0445",
    SUPERTRAIT = "L0446",
    ASSOCIATED_TYPE = "L0447",
    OBJECT_SAFETY = "L0448",
    TRAIT_OUTSIDE_BOX = "L0449",
    MISSING_ASSOCIATED_VALUE = "L0450",
    PROJECTION_WITHOUT_IMPLEMENTOR = "L0451",
    UNDECLARED_ASSOCIATED_VALUE = "L0452",
    PROJECTION_WITHOUT_BOUND = "L0453",
    REPEATED_FIELD = "L0454",
    MAIN_RETURNS_VALUE = "L0455",
    SELFLESS_METHOD_CALL = "L0456",
    UNKNOWN_COLLECTION = "L0457",
    UNNAMEABLE_TYPE = "L0458",

    USE_AFTER_MOVE = "L0501",
    NEEDS_OWNERSHIP = "L0503",
    MISSING_BANG = "L0504",
    MISSING_COPY_BOUND = "L0505",
    WRITTEN_MODE_MISMATCH = "L0506",
    FUNCTION_VALUE_MISMATCH = "L0507",
    CLOSURE_KIND_MISMATCH = "L0509",
    LOCAL_BORROW = "L0510",
    CAPTURING_FUNCTION_FIELD = "L0511",

    MISSING_MAIN = "L0601",
    NAME_COLLISION = "L0602",
    BAD_TEMPLATE = "L0603",
    RUST_ERROR = "L0604",
}

impl DiagnosticCode {
    const STAGE_PREFIX_LENGTH: usize = 3;

    pub fn from_rustc(code: String) -> Self {
        Self(Cow::Owned(code))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn macro_stage_named(name: &str) -> Option<Self> {
        Self::macro_stage_codes()
            .find(|(source_name, _)| source_name == name)
            .map(|(_, code)| code)
    }

    pub fn macro_stage_names() -> Vec<Rc<str>> {
        Self::macro_stage_codes()
            .map(|(source_name, _)| Rc::from(source_name))
            .collect()
    }

    fn macro_stage_codes() -> impl Iterator<Item = (String, Self)> {
        Self::NAMED
            .iter()
            .map(|(constant, code)| (Self::source_name(constant), Self(Cow::Borrowed(code))))
            .filter(|(_, code)| code.stage() == Self::MACRO_FAILED.stage())
    }

    fn source_name(constant: &str) -> String {
        constant.replace('_', "-").to_lowercase()
    }

    fn stage(&self) -> &str {
        &self.0[..Self::STAGE_PREFIX_LENGTH]
    }
}
