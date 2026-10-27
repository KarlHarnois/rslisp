mod support;

use std::path::PathBuf;

use support::{Fixture, FixtureReports};

#[test]
fn reader_fixtures() {
    let mut paths = Vec::new();
    insta::glob!("fixtures/read/*.rslisp", |path| paths
        .push(PathBuf::from(path)));
    let reports = FixtureReports::compute(paths, |path| Fixture::load(path).read_report());
    insta::glob!("fixtures/read/*.rslisp", |path| {
        insta::assert_snapshot!(reports.report(path));
    });
}

#[test]
fn expand_fixtures() {
    let mut paths = Vec::new();
    insta::glob!("fixtures/expand/*.rslisp", |path| paths
        .push(PathBuf::from(path)));
    let reports = FixtureReports::compute(paths, |path| Fixture::load(path).expand_report());
    insta::glob!("fixtures/expand/*.rslisp", |path| {
        insta::assert_snapshot!(reports.report(path));
    });
}

#[test]
fn check_fixtures() {
    let mut paths = Vec::new();
    insta::glob!("fixtures/check/*.rslisp", |path| paths
        .push(PathBuf::from(path)));
    let reports = FixtureReports::compute(paths, |path| Fixture::load(path).check_report());
    insta::glob!("fixtures/check/*.rslisp", |path| {
        insta::assert_snapshot!(reports.report(path));
    });
}

#[test]
fn emit_fixtures() {
    let mut paths = Vec::new();
    insta::glob!("fixtures/emit/*.rslisp", |path| paths
        .push(PathBuf::from(path)));
    let reports = FixtureReports::compute(paths, |path| Fixture::load(path).emit_report());
    insta::glob!("fixtures/emit/*.rslisp", |path| {
        insta::assert_snapshot!(reports.report(path));
    });
}

#[test]
fn run_fixtures() {
    let mut paths = Vec::new();
    insta::glob!("fixtures/run/*.rslisp", |path| paths
        .push(PathBuf::from(path)));
    let reports = FixtureReports::compute(paths, |path| Fixture::load(path).run_report());
    insta::glob!("fixtures/run/*.rslisp", |path| {
        insta::assert_snapshot!(reports.report(path));
    });
}
