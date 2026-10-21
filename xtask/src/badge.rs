use crate::coverage::LineCoverage;

pub struct CoverageBadge {
    shown: Option<String>,
}

impl CoverageBadge {
    const PREFIX: &str = "![line coverage](https://img.shields.io/badge/line%20coverage-";
    const SUFFIX: &str = "%25-brightgreen)";

    pub fn find(readme: &str) -> Self {
        Self {
            shown: readme
                .lines()
                .find(|line| line.starts_with(Self::PREFIX))
                .map(str::to_string),
        }
    }

    pub fn expected(coverage: LineCoverage) -> String {
        format!(
            "{}{}{}",
            Self::PREFIX,
            Self::whole_percent(coverage),
            Self::SUFFIX
        )
    }

    pub fn whole_percent(coverage: LineCoverage) -> u32 {
        coverage.percent().floor() as u32
    }

    pub fn shows(&self, coverage: LineCoverage) -> bool {
        self.shown.as_deref() == Some(Self::expected(coverage).as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::CoverageBadge;
    use crate::coverage::LineCoverage;

    struct SampleCoverage;

    impl SampleCoverage {
        fn percent(covered: usize, total: usize) -> LineCoverage {
            (0..total).map(|line| line < covered).collect()
        }
    }

    #[test]
    fn the_badge_shows_coverage_rounded_down() {
        let coverage = SampleCoverage::percent(995, 1000);
        assert!(CoverageBadge::expected(coverage).contains("coverage-99%25"));
    }

    #[test]
    fn a_readme_with_the_expected_badge_is_current() {
        let coverage = SampleCoverage::percent(995, 1000);
        let readme = format!("# Title\n\n{}\n", CoverageBadge::expected(coverage));
        assert!(CoverageBadge::find(&readme).shows(coverage));
    }

    #[test]
    fn a_readme_with_another_percentage_is_stale() {
        let shown = SampleCoverage::percent(980, 1000);
        let readme = CoverageBadge::expected(shown);
        assert!(!CoverageBadge::find(&readme).shows(SampleCoverage::percent(995, 1000)));
    }

    #[test]
    fn a_readme_without_a_badge_is_stale() {
        let coverage = SampleCoverage::percent(1, 1);
        assert!(!CoverageBadge::find("# Title\n").shows(coverage));
    }
}
