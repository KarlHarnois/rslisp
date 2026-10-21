use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::checks::{BadgeFreshness, Check, CoverageFloor, CrapLimit};
use crate::crap::ScoredFunction;
use crate::failure::GateFailure;
use crate::functions::{SourceFunction, SourceTree};
use crate::instrumented_tests::InstrumentedTests;
use crate::lcov::LcovReport;

pub struct QualityGate {
    root: PathBuf,
}

struct Measurement {
    report: LcovReport,
    functions: Vec<SourceFunction>,
    readme: String,
}

impl QualityGate {
    const MINIMUM_LINE_COVERAGE: f64 = 96.0;
    const MAXIMUM_CRAP: f64 = 42.0;
    const README: &str = "README.md";

    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    pub fn run(&self) -> ExitCode {
        match self.measure() {
            Ok(measurement) => Self::judge(measurement),
            Err(failure) => failure.report(),
        }
    }

    fn measure(&self) -> Result<Measurement, GateFailure> {
        let report = InstrumentedTests::new(&self.root).run()?;
        let functions = SourceTree::new(&self.root).functions()?;
        let readme_path = self.root.join(Self::README);
        let readme = fs::read_to_string(&readme_path)
            .map_err(|error| GateFailure::unreadable(&readme_path, error))?;
        Ok(Measurement {
            report,
            functions,
            readme,
        })
    }

    fn judge(measurement: Measurement) -> ExitCode {
        let coverage = measurement.report.total();
        let scored = Self::score(measurement.functions, &measurement.report);
        let checks: [&dyn Check; 3] = [
            &CoverageFloor::new(Self::MINIMUM_LINE_COVERAGE, &measurement.report),
            &CrapLimit::new(Self::MAXIMUM_CRAP, scored),
            &BadgeFreshness::new(&measurement.readme, coverage),
        ];
        let failures = checks
            .into_iter()
            .filter(|check| !Self::announce(*check))
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
