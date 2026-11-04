pub struct Keyword;

impl Keyword {
    pub const MARKER: &'static str = ":";

    pub fn field_name(symbol_name: &str) -> Option<&str> {
        symbol_name
            .strip_prefix(Self::MARKER)
            .filter(|field_name| !field_name.is_empty())
    }

    pub fn of_field(field_name: &str) -> Option<String> {
        (!field_name.starts_with(Self::MARKER)).then(|| format!("{}{field_name}", Self::MARKER))
    }

    pub fn trim_end(symbol_name: &str) -> Option<String> {
        let field_name = Self::field_name(symbol_name)?;
        let trimmed = field_name.trim_end_matches(Self::MARKER);
        Some(format!("{}{trimmed}", Self::MARKER))
    }
}

#[cfg(test)]
mod tests {
    use super::Keyword;

    #[test]
    fn trim_end_drops_every_trailing_marker() {
        assert_eq!(Keyword::trim_end(":email::").as_deref(), Some(":email"));
    }

    #[test]
    fn trim_end_keeps_a_keyword_without_a_trailing_marker() {
        assert_eq!(Keyword::trim_end(":email").as_deref(), Some(":email"));
    }

    #[test]
    fn trim_end_leaves_only_the_marker_of_a_keyword_made_of_markers() {
        assert_eq!(Keyword::trim_end(":::").as_deref(), Some(":"));
    }

    #[test]
    fn trim_end_rejects_a_plain_symbol() {
        assert_eq!(Keyword::trim_end("email:"), None);
    }
}
