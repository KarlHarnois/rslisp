mod support;

use std::path::PathBuf;

use support::{Fixture, FixtureReports};

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
