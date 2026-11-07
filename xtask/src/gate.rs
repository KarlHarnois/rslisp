use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::checks::{BadgeFreshness, Check, CleanEmittedRust, CoverageFloor, CrapLimit};
use crate::crap::ScoredFunction;
use crate::emitted_lints::{EmittedLints, LintedProgram};
use crate::failure::TaskFailure;
use crate::functions::{SourceFunction, SourceTree};
use crate::instrumented_tests::InstrumentedTests;
use crate::lcov::LcovReport;
use crate::workspace::CargoWorkspace;

pub struct QualityGate {
    root: PathBuf,
}

struct CoverageEvidence {
    report: LcovReport,
    functions: Vec<SourceFunction>,
    readme: String,
}

impl QualityGate {
    const MINIMUM_LINE_COVERAGE: f64 = 99.0;
    const MAXIMUM_CRAP: f64 = 8.0;
    const README: &str = "README.md";

    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    pub fn run(&self) -> Result<ExitCode, TaskFailure> {
        let (evidence, linted) = self.measure_everything()?;
        Ok(Self::judge(evidence, &[&CleanEmittedRust::new(linted)]))
    }

    pub fn run_on_shards(&self) -> Result<ExitCode, TaskFailure> {
        let report = InstrumentedTests::new(&self.root).merge_shards()?;
        Ok(Self::judge(self.evidence(report)?, &[]))
    }

    pub fn run_emitted_lints(&self) -> Result<ExitCode, TaskFailure> {
        let linted = EmittedLints::new(&self.root).run()?;
        Ok(Self::conclude(&[&CleanEmittedRust::new(linted)]))
    }

    fn measure_everything(&self) -> Result<(CoverageEvidence, Vec<LintedProgram>), TaskFailure> {
        let report = InstrumentedTests::new(&self.root).run()?;
        let evidence = self.evidence(report)?;
        let linted = EmittedLints::new(&self.root).run()?;
        Ok((evidence, linted))
    }

    fn evidence(&self, report: LcovReport) -> Result<CoverageEvidence, TaskFailure> {
        let directories = CargoWorkspace::read(&self.root)?.source_directories();
        let functions = SourceTree::new(&self.root, directories).functions()?;
        let readme_path = self.root.join(Self::README);
        let readme = fs::read_to_string(&readme_path)
            .map_err(|error| TaskFailure::unreadable(&readme_path, error))?;
        Ok(CoverageEvidence {
            report,
            functions,
            readme,
        })
    }

    fn judge(evidence: CoverageEvidence, further_checks: &[&dyn Check]) -> ExitCode {
        let coverage = evidence.report.total();
        let scored = Self::score(evidence.functions, &evidence.report);
        let floor = CoverageFloor::new(Self::MINIMUM_LINE_COVERAGE, &evidence.report);
        let crap = CrapLimit::new(Self::MAXIMUM_CRAP, scored);
        let badge = BadgeFreshness::new(&evidence.readme, coverage);
        let coverage_checks: [&dyn Check; 3] = [&floor, &crap, &badge];
        Self::conclude(&[&coverage_checks[..], further_checks].concat())
    }

    fn conclude(checks: &[&dyn Check]) -> ExitCode {
        let failures = checks
            .iter()
            .filter(|check| !Self::announce(**check))
            .count();
        if failures == 0 {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }

    fn score(functions: Vec<SourceFunction>, report: &LcovReport) -> Vec<ScoredFunction> {
        functions
            .into_iter()
            .map(|function| {
                let coverage = report.within(&function.file, function.lines);
                ScoredFunction::new(function, coverage)
            })
            .collect()
    }

    fn announce(check: &dyn Check) -> bool {
        let passes = check.passes();
        if passes {
            println!("ok: {}", check.summary());
        } else {
            println!("error: {}\n{}", check.summary(), check.details());
        }
        passes
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::ExitCode;

    use super::{CoverageEvidence, QualityGate};
    use crate::badge::CoverageBadge;
    use crate::checks::Check;
    use crate::lcov::LcovReport;

    struct FixedCheck {
        passes: bool,
    }

    impl Check for FixedCheck {
        fn passes(&self) -> bool {
            self.passes
        }

        fn summary(&self) -> String {
            "a fixed check".to_string()
        }

        fn details(&self) -> String {
            String::new()
        }
    }

    impl CoverageEvidence {
        fn fully_covered() -> Self {
            let report = LcovReport::parse("SF:/work/src/a.rs\nDA:1,1\n", Path::new("/work"));
            let readme = CoverageBadge::expected(report.total());
            Self {
                report,
                functions: Vec::new(),
                readme,
            }
        }
    }

    #[test]
    fn covered_evidence_passes_with_passing_further_checks() {
        let passing = FixedCheck { passes: true };
        let verdict = QualityGate::judge(CoverageEvidence::fully_covered(), &[&passing]);
        assert_eq!(verdict, ExitCode::SUCCESS);
    }

    #[test]
    fn a_failing_further_check_fails_the_gate() {
        let failing = FixedCheck { passes: false };
        let verdict = QualityGate::judge(CoverageEvidence::fully_covered(), &[&failing]);
        assert_eq!(verdict, ExitCode::FAILURE);
    }
}
