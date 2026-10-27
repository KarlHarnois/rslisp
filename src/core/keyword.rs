pub struct Keyword;

impl Keyword {
    pub const MARKER: &'static str = ":";

    pub fn field_name(symbol_name: &str) -> Option<&str> {
        symbol_name
            .strip_prefix(Self::MARKER)
            .filter(|field_name| !field_name.is_empty())
    }
}
