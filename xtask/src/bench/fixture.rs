pub struct HeadedResultsFixture;

impl HeadedResultsFixture {
    pub const METHOD_KEY: &'static str = "1111111-2222222-3333333-4444444";
    pub const TEXT: &'static str = include_str!("../../fixtures/bench/headed-results.tsv");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::results::ResultSet;

    #[test]
    fn names_the_method_key_of_the_headed_results() {
        let results = ResultSet::parse(HeadedResultsFixture::TEXT).unwrap();
        assert_eq!(results.method_key(), Some(HeadedResultsFixture::METHOD_KEY));
        assert_eq!(results.programs().len(), 2);
    }
}
