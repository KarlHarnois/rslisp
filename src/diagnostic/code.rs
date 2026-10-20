#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagnosticCode(&'static str);

impl DiagnosticCode {
    pub const UNCLOSED_LIST: Self = Self("L0101");
    pub const UNMATCHED_CLOSE: Self = Self("L0102");
    pub const UNTERMINATED_STRING: Self = Self("L0103");
    pub const UNKNOWN_ESCAPE: Self = Self("L0104");
    pub const MALFORMED_NUMBER: Self = Self("L0105");
    pub const RESERVED_CHARACTER: Self = Self("L0106");

    pub fn as_str(self) -> &'static str {
        self.0
    }
}
