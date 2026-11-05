use std::ops::RangeInclusive;

use crate::reader::{Form, FormKind, Reader};
use crate::source::{SourceFile, SourceId};

#[derive(Debug, PartialEq, Eq)]
pub enum TestRecord {
    Untested(UntestedNames),
    Start(usize),
    Test(TestName),
    Ok,
    Fail(TestFailure),
    Panic(TestPanic),
    Done,
}

#[derive(Debug, PartialEq, Eq)]
pub struct UntestedNames {
    pub module: String,
    pub exports: usize,
    pub names: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct TestName {
    pub name: String,
    pub summary: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct TestFailure {
    pub location: String,
    pub written: String,
    pub sides: Option<(String, String)>,
    pub message: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct TestPanic {
    pub location: String,
    pub message: String,
}

type RecordParser = fn(&RecordParts) -> Option<TestRecord>;

struct RecordParts {
    head: String,
    parts: Vec<Form>,
}

impl TestRecord {
    const SOURCE_NAME: &'static str = "<test record>";

    pub fn parse(text: &str) -> Option<Self> {
        let file = SourceFile::new(
            SourceId::new(0),
            Self::SOURCE_NAME.to_string(),
            text.to_string(),
        );
        let forms = Reader::new(&file).read().ok()?;
        let [form] = <[Form; 1]>::try_from(forms).ok()?;
        RecordParts::of(&form)?.record()
    }
}

impl RecordParts {
    const MESSAGE_POSITION: usize = 2;
    const EQUAL_MESSAGE_POSITION: usize = 4;

    fn of(form: &Form) -> Option<Self> {
        let (head, parts) = form.as_list()?.split_first()?;
        Some(Self {
            head: head.as_symbol()?.name().to_string(),
            parts: parts.to_vec(),
        })
    }

    const PARSERS: [(&'static str, RecordParser); 8] = [
        ("untested", Self::untested),
        ("start", Self::start),
        ("test", Self::test_name),
        ("ok", Self::ok),
        ("fail", Self::failure),
        ("fail-equal", Self::equal_failure),
        ("panic", Self::panic),
        ("done", Self::done),
    ];

    fn record(&self) -> Option<TestRecord> {
        let (_, parse) = Self::PARSERS.iter().find(|(head, _)| *head == self.head)?;
        parse(self)
    }

    fn untested(&self) -> Option<TestRecord> {
        self.expect_parts(3..=3)?;
        Some(TestRecord::Untested(UntestedNames {
            module: self.text(0)?,
            exports: self.count(1)?,
            names: self.text(2)?,
        }))
    }

    fn start(&self) -> Option<TestRecord> {
        self.expect_parts(1..=1)?;
        Some(TestRecord::Start(self.count(0)?))
    }

    fn test_name(&self) -> Option<TestRecord> {
        self.expect_parts(2..=2)?;
        Some(TestRecord::Test(TestName {
            name: self.text(0)?,
            summary: self.text(1)?,
        }))
    }

    fn ok(&self) -> Option<TestRecord> {
        self.expect_parts(0..=0)?;
        Some(TestRecord::Ok)
    }

    fn failure(&self) -> Option<TestRecord> {
        self.expect_parts(2..=3)?;
        Some(TestRecord::Fail(TestFailure {
            location: self.text(0)?,
            written: self.text(1)?,
            sides: None,
            message: self.text(Self::MESSAGE_POSITION),
        }))
    }

    fn equal_failure(&self) -> Option<TestRecord> {
        self.expect_parts(4..=5)?;
        Some(TestRecord::Fail(TestFailure {
            location: self.text(0)?,
            written: self.text(1)?,
            sides: Some(self.sides()?),
            message: self.text(Self::EQUAL_MESSAGE_POSITION),
        }))
    }

    fn panic(&self) -> Option<TestRecord> {
        self.expect_parts(2..=2)?;
        Some(TestRecord::Panic(TestPanic {
            location: self.text(0)?,
            message: self.text(1)?,
        }))
    }

    fn done(&self) -> Option<TestRecord> {
        self.expect_parts(0..=0)?;
        Some(TestRecord::Done)
    }

    fn expect_parts(&self, counts: RangeInclusive<usize>) -> Option<()> {
        counts.contains(&self.parts.len()).then_some(())
    }

    fn sides(&self) -> Option<(String, String)> {
        Some((self.text(2)?, self.text(3)?))
    }

    fn text(&self, position: usize) -> Option<String> {
        match self.parts.get(position)?.kind() {
            FormKind::String(text) => Some(text.to_string()),
            _ => None,
        }
    }

    fn count(&self, position: usize) -> Option<usize> {
        match self.parts.get(position)?.kind() {
            FormKind::Number(number) => number.text().parse().ok(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TestFailure, TestName, TestPanic, TestRecord, UntestedNames};

    struct RecordFixture;

    impl RecordFixture {
        fn parsed(text: &str) -> Option<TestRecord> {
            TestRecord::parse(text)
        }

        fn failure(sides: Option<(&str, &str)>, message: Option<&str>) -> TestRecord {
            TestRecord::Fail(TestFailure {
                location: "pads.rslisp:3:3".to_string(),
                written: "(assert (= (pad \"a\") \"a\"))".to_string(),
                sides: sides.map(|(left, right)| (left.to_string(), right.to_string())),
                message: message.map(str::to_string),
            })
        }
    }

    #[test]
    fn a_start_record_carries_the_count_of_selected_tests() {
        assert_eq!(
            RecordFixture::parsed("(start 3)"),
            Some(TestRecord::Start(3))
        );
    }

    #[test]
    fn a_test_record_carries_the_name_and_the_summary() {
        assert_eq!(
            RecordFixture::parsed("(test \"pads/pads-left\" \"Pads.\")"),
            Some(TestRecord::Test(TestName {
                name: "pads/pads-left".to_string(),
                summary: "Pads.".to_string(),
            }))
        );
    }

    #[test]
    fn a_fail_record_reads_its_escaped_written_assert() {
        let text = "(fail \"pads.rslisp:3:3\" \"(assert (= (pad \\\"a\\\") \\\"a\\\"))\")";
        assert_eq!(
            RecordFixture::parsed(text),
            Some(RecordFixture::failure(None, None))
        );
    }

    #[test]
    fn a_fail_equal_record_carries_both_sides_and_a_message() {
        let text = "(fail-equal \"pads.rslisp:3:3\" \"(assert (= (pad \\\"a\\\") \\\"a\\\"))\" \"\\\" a\\\"\" \"\\\"a\\\"\" \"pads\")";
        assert_eq!(
            RecordFixture::parsed(text),
            Some(RecordFixture::failure(
                Some(("\" a\"", "\"a\"")),
                Some("pads")
            ))
        );
    }

    #[test]
    fn a_panic_record_carries_the_location_and_the_message() {
        assert_eq!(
            RecordFixture::parsed("(panic \"pads.rslisp:2:1\" \"boom\")"),
            Some(TestRecord::Panic(TestPanic {
                location: "pads.rslisp:2:1".to_string(),
                message: "boom".to_string(),
            }))
        );
    }

    #[test]
    fn an_untested_record_carries_the_module_the_count_and_the_names() {
        assert_eq!(
            RecordFixture::parsed("(untested \"prelude\" 221 \"len pad\")"),
            Some(TestRecord::Untested(UntestedNames {
                module: "prelude".to_string(),
                exports: 221,
                names: "len pad".to_string(),
            }))
        );
    }

    #[test]
    fn the_records_without_parts_are_read() {
        assert_eq!(RecordFixture::parsed("(ok)"), Some(TestRecord::Ok));
        assert_eq!(RecordFixture::parsed("(done)"), Some(TestRecord::Done));
    }

    #[test]
    fn a_record_with_the_wrong_parts_is_not_read() {
        assert_eq!(RecordFixture::parsed("(start \"three\")"), None);
        assert_eq!(RecordFixture::parsed("(ok 1)"), None);
        assert_eq!(RecordFixture::parsed("(finish)"), None);
        assert_eq!(RecordFixture::parsed("start"), None);
        assert_eq!(RecordFixture::parsed("(ok) (ok)"), None);
        assert_eq!(RecordFixture::parsed("((ok))"), None);
    }
}
