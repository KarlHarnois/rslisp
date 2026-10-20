mod support;

use support::Fixture;

#[test]
fn reader_fixtures() {
    insta::glob!("fixtures/read/*.rslisp", |path| {
        insta::assert_snapshot!(Fixture::load(path).read_report());
    });
}

#[test]
fn expand_fixtures() {
    insta::glob!("fixtures/expand/*.rslisp", |path| {
        insta::assert_snapshot!(Fixture::load(path).expand_report());
    });
}

#[test]
fn check_fixtures() {
    insta::glob!("fixtures/check/*.rslisp", |path| {
        insta::assert_snapshot!(Fixture::load(path).check_report());
    });
}

#[test]
fn emit_fixtures() {
    insta::glob!("fixtures/emit/*.rslisp", |path| {
        insta::assert_snapshot!(Fixture::load(path).emit_report());
    });
}

#[test]
fn run_fixtures() {
    insta::glob!("fixtures/run/*.rslisp", |path| {
        insta::assert_snapshot!(Fixture::load(path).run_report());
    });
}
