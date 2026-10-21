use crate::coverage::LineCoverage;
use crate::functions::SourceFunction;

pub struct ScoredFunction {
    pub function: SourceFunction,
    pub coverage: LineCoverage,
    pub crap: f64,
}

impl ScoredFunction {
    pub fn new(function: SourceFunction, coverage: LineCoverage) -> Self {
        let complexity = function.complexity as f64;
        let crap = complexity * complexity * coverage.uncovered_fraction().powi(3) + complexity;
        Self {
            function,
            coverage,
            crap,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ScoredFunction;
    use crate::coverage::LineCoverage;
    use crate::functions::{LineSpan, SourceFunction};

    struct Scoring;

    impl Scoring {
        fn crap(complexity: usize, covered_lines: &[bool]) -> f64 {
            let function = SourceFunction {
                file: "src/shapes.rs".to_string(),
                name: "area".to_string(),
                lines: LineSpan { first: 1, last: 4 },
                complexity,
            };
            let coverage: LineCoverage = covered_lines.iter().copied().collect();
            ScoredFunction::new(function, coverage).crap
        }
    }

    #[test]
    fn a_fully_covered_function_scores_its_complexity() {
        assert_eq!(Scoring::crap(5, &[true, true]), 5.0);
    }

    #[test]
    fn an_uncovered_function_scores_complexity_squared_plus_complexity() {
        assert_eq!(Scoring::crap(5, &[false, false]), 30.0);
    }

    #[test]
    fn half_coverage_scores_an_eighth_of_complexity_squared_plus_complexity() {
        assert_eq!(Scoring::crap(10, &[true, false]), 22.5);
    }
}
