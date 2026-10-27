use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::checks::{BadgeFreshness, Check, CleanEmittedRust, CoverageFloor, CrapLimit};
use crate::crap::ScoredFunction;
use crate::emitted_lints::{EmittedLints, LintedProgram};
use crate::failure::GateFailure;
use crate::functions::{SourceFunction, SourceTree};
use crate::instrumented_tests::InstrumentedTests;
use crate::lcov::LcovReport;
use crate::platform::Platform;

pub struct QualityGate {
    root: PathBuf,
    platform: Platform,
}

struct Measurement {
    report: LcovReport,
    functions: Vec<SourceFunction>,
    readme: String,
    linted: Vec<LintedProgram>,
}

impl QualityGate {
    const MINIMUM_LINE_COVERAGE: f64 = 99.0;
    const MAXIMUM_CRAP: f64 = 8.0;
    const README: &str = "README.md";

    pub fn new(root: &Path, platform: Platform) -> Self {
        Self {
            root: root.to_path_buf(),
            platform,
        }
    }

    pub fn run(&self) -> ExitCode {
        match self.measure() {
            Ok(measurement) => self.judge(measurement),
            Err(failure) => failure.report(),
        }
    }

    fn measure(&self) -> Result<Measurement, GateFailure> {
        let report = InstrumentedTests::new(&self.root).run()?;
        let functions = SourceTree::new(&self.root).functions()?;
        let readme_path = self.root.join(Self::README);
        let readme = fs::read_to_string(&readme_path)
            .map_err(|error| GateFailure::unreadable(&readme_path, error))?;
        let linted = EmittedLints::new(&self.root).run()?;
        Ok(Measurement {
            report,
            functions,
            readme,
            linted,
        })
    }

    fn judge(&self, measurement: Measurement) -> ExitCode {
        self.announce_skipped_line_coverage();
        let scored = Self::score(measurement.functions, &measurement.report);
        let mut checks = self.line_coverage_checks(&measurement.report, &measurement.readme);
        checks.push(Box::new(CrapLimit::new(Self::MAXIMUM_CRAP, scored)));
        checks.push(Box::new(CleanEmittedRust::new(measurement.linted)));
        let failures = checks
            .iter()
            .filter(|check| !Self::announce(check.as_ref()))
            .count();
        if failures == 0 {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }

    fn line_coverage_checks<'a>(
        &self,
        report: &'a LcovReport,
        readme: &'a str,
    ) -> Vec<Box<dyn Check + 'a>> {
        if !self.platform.judges_line_coverage() {
            return Vec::new();
        }
        vec![
            Box::new(CoverageFloor::new(Self::MINIMUM_LINE_COVERAGE, report)),
            Box::new(BadgeFreshness::new(readme, report.total())),
        ]
    }

    fn announce_skipped_line_coverage(&self) {
        if !self.platform.judges_line_coverage() {
            println!("{}", self.platform.line_coverage_notice());
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
