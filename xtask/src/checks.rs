use crate::badge::CoverageBadge;
use crate::coverage::LineCoverage;
use crate::crap::ScoredFunction;
use crate::emitted_lints::LintedProgram;
use crate::lcov::LcovReport;

pub trait Check {
    fn passes(&self) -> bool;
    fn summary(&self) -> String;
    fn details(&self) -> String;
}

pub struct CoverageFloor<'a> {
    minimum: f64,
    report: &'a LcovReport,
}

impl<'a> CoverageFloor<'a> {
    const LISTED_FILES: usize = 10;

    pub fn new(minimum: f64, report: &'a LcovReport) -> Self {
        Self { minimum, report }
    }
}

impl Check for CoverageFloor<'_> {
    fn passes(&self) -> bool {
        self.report.total().percent() >= self.minimum
    }

    fn summary(&self) -> String {
        format!(
            "line coverage is {:.2}%, and the floor is {}%",
            self.report.total().percent(),
            self.minimum
        )
    }

    fn details(&self) -> String {
        let files: String = self
            .report
            .most_missed(Self::LISTED_FILES)
            .iter()
            .map(|missed| format!("  {missed}\n"))
            .collect();
        format!(
            "The files with the most missed lines:\n{files}\
             If a test target was deleted, run `cargo llvm-cov clean --workspace` once: \
             its stale binary counts as uncovered.\n"
        )
    }
}

pub struct CrapLimit {
    maximum: f64,
    functions: Vec<ScoredFunction>,
}

impl CrapLimit {
    pub fn new(maximum: f64, mut functions: Vec<ScoredFunction>) -> Self {
        functions.sort_by(|left, right| right.crap.total_cmp(&left.crap));
        Self { maximum, functions }
    }

    fn over_limit(&self) -> impl Iterator<Item = &ScoredFunction> {
        self.functions
            .iter()
            .take_while(|scored| scored.crap > self.maximum)
    }
}

impl Check for CrapLimit {
    fn passes(&self) -> bool {
        self.over_limit().next().is_none()
    }

    fn summary(&self) -> String {
        match self.functions.first() {
            Some(worst) => format!(
                "the worst of {} CRAP scores is {:.1}, in {}, and the limit is {}",
                self.functions.len(),
                worst.crap,
                worst.function,
                self.maximum
            ),
            None => "no function to score".to_string(),
        }
    }

    fn details(&self) -> String {
        let rows: String = self
            .over_limit()
            .map(|scored| {
                format!(
                    "  {:>7.1} {:>4} {:>7.1}%  {}\n",
                    scored.crap,
                    scored.function.complexity,
                    scored.coverage.percent(),
                    scored.function
                )
            })
            .collect();
        format!(
            "Split these functions or test their branches:\n     CRAP   CC  covered  function\n{rows}"
        )
    }
}

pub struct BadgeFreshness<'a> {
    readme: &'a str,
    coverage: LineCoverage,
}

impl<'a> BadgeFreshness<'a> {
    pub fn new(readme: &'a str, coverage: LineCoverage) -> Self {
        Self { readme, coverage }
    }
}

impl Check for BadgeFreshness<'_> {
    fn passes(&self) -> bool {
        CoverageBadge::find(self.readme).shows(self.coverage)
    }

    fn summary(&self) -> String {
        let verb = if self.passes() { "shows" } else { "must show" };
        format!(
            "the README coverage badge {verb} {}%",
            CoverageBadge::whole_percent(self.coverage)
        )
    }

    fn details(&self) -> String {
        format!(
            "Put this line under the title of README.md, in place of any older badge:\n  {}\n",
            CoverageBadge::expected(self.coverage)
        )
    }
}

pub struct CleanEmittedRust {
    programs: Vec<LintedProgram>,
}

impl CleanEmittedRust {
    pub fn new(programs: Vec<LintedProgram>) -> Self {
        Self { programs }
    }
}

impl Check for CleanEmittedRust {
    fn passes(&self) -> bool {
        self.programs.is_empty()
    }

    fn summary(&self) -> String {
        format!(
            "clippy warns on the emitted Rust of {} emit fixtures",
            self.programs.len()
        )
    }

    fn details(&self) -> String {
        let programs: String = self
            .programs
            .iter()
            .map(|program| format!("{}:\n{}\n", program.fixture, program.warnings))
            .collect();
        format!(
            "{programs}Fix the emitter. A lint about the program's own choices belongs in \
             `EmittedLints::PROGRAM_CHOICES`, with the project owner's approval.\n"
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{BadgeFreshness, Check, CleanEmittedRust, CoverageFloor, CrapLimit};
    use crate::coverage::LineCoverage;
    use crate::crap::ScoredFunction;
    use crate::emitted_lints::LintedProgram;
    use crate::functions::{LineSpan, SourceFunction};
    use crate::lcov::LcovReport;

    struct Samples;

    impl Samples {
        fn report() -> LcovReport {
            let text = "SF:/work/src/main.rs\nDA:1,1\nDA:2,1\nDA:3,1\nDA:4,0\nend_of_record\n";
            LcovReport::parse(text, Path::new("/work"))
        }

        fn scored(name: &str, complexity: usize) -> ScoredFunction {
            let function = SourceFunction {
                file: "src/main.rs".to_string(),
                name: name.to_string(),
                lines: LineSpan { first: 1, last: 4 },
                complexity,
            };
            ScoredFunction::new(function, [true].into_iter().collect::<LineCoverage>())
        }
    }

    #[test]
    fn coverage_at_the_floor_passes() {
        let report = Samples::report();
        assert!(CoverageFloor::new(75.0, &report).passes());
        assert!(!CoverageFloor::new(76.0, &report).passes());
    }

    #[test]
    fn coverage_details_list_missed_lines() {
        let report = Samples::report();
        assert!(
            CoverageFloor::new(80.0, &report)
                .details()
                .contains("src/main.rs: 4")
        );
    }

    #[test]
    fn crap_limit_fails_on_the_functions_over_it() {
        let limit = CrapLimit::new(
            8.0,
            vec![Samples::scored("simple", 2), Samples::scored("branchy", 9)],
        );
        assert!(!limit.passes());
        assert!(limit.summary().contains("9.0, in src/main.rs:1 branchy"));
        assert!(limit.details().contains("branchy"));
        assert!(!limit.details().contains("simple"));
    }

    #[test]
    fn crap_limit_passes_when_every_function_is_within_it() {
        assert!(CrapLimit::new(8.0, vec![Samples::scored("simple", 8)]).passes());
    }

    #[test]
    fn badge_freshness_names_the_line_to_write() {
        let coverage = Samples::report().total();
        let check = BadgeFreshness::new("# Title\n", coverage);
        assert!(!check.passes());
        assert!(check.details().contains("line%20coverage-75%25"));
    }

    #[test]
    fn clean_emitted_rust_passes_without_warnings() {
        assert!(CleanEmittedRust::new(Vec::new()).passes());
    }

    #[test]
    fn clean_emitted_rust_names_each_warned_fixture() {
        let check = CleanEmittedRust::new(vec![LintedProgram {
            fixture: "person".to_string(),
            warnings: "warning: unused variable".to_string(),
        }]);
        assert!(!check.passes());
        assert!(
            check
                .details()
                .contains("person:\nwarning: unused variable")
        );
    }
}
