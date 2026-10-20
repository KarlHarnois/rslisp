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
