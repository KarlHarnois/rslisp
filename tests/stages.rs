mod support;

use std::path::PathBuf;

use rslisp::expand::PreludeState;
use support::{Fixture, FixtureReports, HeaderAgreement};

#[test]
fn reader_fixtures() {
    let fixtures = "fixtures/read/*.rslisp";
    let mut paths = Vec::new();
    insta::glob!(fixtures, |path| paths.push(PathBuf::from(path)));
    let reports = FixtureReports::compute(paths, |path| Fixture::load(path).read_report());
    insta::glob!(fixtures, |path| {
        insta::assert_snapshot!(reports.report(path));
    });
}

#[test]
fn expand_fixtures() {
    let fixtures = "fixtures/expand/*.rslisp";
    let mut paths = Vec::new();
    insta::glob!(fixtures, |path| paths.push(PathBuf::from(path)));
    let reports = FixtureReports::compute(paths, |path| Fixture::load(path).expand_report());
    insta::glob!(fixtures, |path| {
        insta::assert_snapshot!(reports.report(path));
    });
}

#[test]
fn check_fixtures() {
    let fixtures = "fixtures/check/*.rslisp";
    let mut paths = Vec::new();
    insta::glob!(fixtures, |path| paths.push(PathBuf::from(path)));
    let reports = FixtureReports::compute(paths, |path| Fixture::load(path).check_report());
    insta::glob!(fixtures, |path| {
        insta::assert_snapshot!(reports.report(path));
    });
}

#[test]
fn emit_fixtures() {
    let fixtures = "fixtures/emit/*.rslisp";
    let mut paths = Vec::new();
    insta::glob!(fixtures, |path| paths.push(PathBuf::from(path)));
    let reports = FixtureReports::compute(paths, |path| Fixture::load(path).emit_report());
    insta::glob!(fixtures, |path| {
        insta::assert_snapshot!(reports.report(path));
    });
}

#[test]
fn run_fixtures() {
    let fixtures = "fixtures/run/*.rslisp";
    let mut paths = Vec::new();
    insta::glob!(fixtures, |path| paths.push(PathBuf::from(path)));
    let reports = FixtureReports::compute(paths, |path| Fixture::load(path).run_report());
    insta::glob!(fixtures, |path| {
        insta::assert_snapshot!(reports.report(path));
    });
}

#[test]
fn corpus_fixtures() {
    let fixtures = "fixtures/corpus/*.rslisp";
    let mut paths = Vec::new();
    insta::glob!(fixtures, |path| paths.push(PathBuf::from(path)));
    let reports = FixtureReports::compute(paths, |path| Fixture::load(path).run_report());
    insta::glob!(fixtures, |path| {
        insta::assert_snapshot!(reports.report(path));
    });
}

#[test]
fn defn_headers_agree() {
    HeaderAgreement::of("defn").assert_headers_agree();
}

#[test]
fn defmacro_headers_agree() {
    HeaderAgreement::of("defmacro").assert_headers_agree();
}

#[test]
fn defstruct_headers_agree() {
    HeaderAgreement::of("defstruct").assert_headers_agree();
}

#[test]
fn defrule_headers_agree() {
    HeaderAgreement::of("defrule").assert_headers_agree();
}

#[test]
fn saved_prelude_fixtures() {
    let mut paths = Vec::new();
    insta::glob!("fixtures/{expand,check,emit,run,corpus}/*.rslisp", |path| {
        paths.push(PathBuf::from(path))
    });
    let saved = PreludeState::build().save();
    let built = FixtureReports::compute(paths.clone(), |path| Fixture::load(path).stage_report());
    let loaded = FixtureReports::compute(paths.clone(), |path| {
        let prelude = PreludeState::load(&saved).expect("the saved prelude loads");
        Fixture::load(path)
            .with_saved_prelude(prelude)
            .stage_report()
    });
    for path in &paths {
        assert_eq!(
            loaded.report(path),
            built.report(path),
            "a loaded prelude changes {}",
            path.display()
        );
    }
}
