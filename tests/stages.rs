mod support;

use std::path::PathBuf;

use support::{Fixture, FixtureReports, FormatFailures, HeaderAgreement, SavedPreludeAgreement};

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
fn project_fixtures() {
    let fixtures = "fixtures/project/*.rslisp";
    let mut paths = Vec::new();
    insta::glob!(fixtures, |path| paths.push(PathBuf::from(path)));
    let reports = FixtureReports::compute(paths, |path| Fixture::load(path).project_report());
    insta::glob!(fixtures, |path| {
        insta::assert_snapshot!(reports.report(path));
    });
}

#[test]
fn format_fixtures() {
    let fixtures = "fixtures/format/*.rslisp";
    let mut paths = Vec::new();
    insta::glob!(fixtures, |path| paths.push(PathBuf::from(path)));
    let reports = FixtureReports::compute(paths, |path| Fixture::load(path).format_report());
    insta::glob!(fixtures, |path| {
        insta::assert_snapshot!(reports.report(path));
    });
}

#[test]
fn every_repository_source_formats_to_the_same_forms_and_stays_formatted() {
    let failures = FormatFailures::of_repository();
    eprintln!(
        "formatted {} files in {:?}",
        failures.formatted, failures.elapsed
    );
    assert!(
        failures.failed.is_empty(),
        "the formatter fails its checks on:\n{}",
        failures.failed.join("\n")
    );
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
fn saved_prelude_expand_fixtures() {
    let mut paths = Vec::new();
    insta::glob!("fixtures/expand/*.rslisp", |path| paths
        .push(PathBuf::from(path)));
    SavedPreludeAgreement::of(paths).assert_loaded_prelude_agrees();
}

#[test]
fn saved_prelude_check_fixtures() {
    let mut paths = Vec::new();
    insta::glob!("fixtures/check/*.rslisp", |path| paths
        .push(PathBuf::from(path)));
    SavedPreludeAgreement::of(paths).assert_loaded_prelude_agrees();
}

#[test]
fn saved_prelude_emit_fixtures() {
    let mut paths = Vec::new();
    insta::glob!("fixtures/emit/*.rslisp", |path| paths
        .push(PathBuf::from(path)));
    SavedPreludeAgreement::of(paths).assert_loaded_prelude_agrees();
}

#[test]
fn saved_prelude_run_fixtures() {
    let mut paths = Vec::new();
    insta::glob!("fixtures/run/*.rslisp", |path| paths
        .push(PathBuf::from(path)));
    SavedPreludeAgreement::of(paths).assert_loaded_prelude_agrees();
}

#[test]
fn saved_prelude_corpus_fixtures() {
    let mut paths = Vec::new();
    insta::glob!("fixtures/corpus/*.rslisp", |path| paths
        .push(PathBuf::from(path)));
    SavedPreludeAgreement::of(paths).assert_loaded_prelude_agrees();
}
