pub struct Counted;

impl Counted {
    pub fn noun(count: usize, singular: &str) -> String {
        if count == 1 {
            format!("{count} {singular}")
        } else {
            format!("{count} {singular}s")
        }
    }

    pub fn was_or_were(count: usize) -> &'static str {
        if count == 1 { "was" } else { "were" }
    }
}

#[cfg(test)]
mod tests {
    use super::Counted;

    #[test]
    fn one_item_is_singular() {
        assert_eq!(Counted::noun(1, "argument"), "1 argument");
        assert_eq!(Counted::was_or_were(1), "was");
    }

    #[test]
    fn other_counts_are_plural() {
        assert_eq!(Counted::noun(0, "argument"), "0 arguments");
        assert_eq!(Counted::was_or_were(2), "were");
    }
}
