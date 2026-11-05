use std::fmt::Display;
use std::io::{self, Write};

use super::test_record::{TestFailure, TestName, TestPanic, TestRecord, UntestedNames};

pub struct TestReport<W: Write> {
    output: W,
    counts: TestCounts,
    running: Option<RunningTest>,
    finished: bool,
}

#[derive(Default)]
struct TestCounts {
    selected: usize,
    passed: usize,
    failed: usize,
}

struct RunningTest {
    name: TestName,
    output: TestOutput,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TestOutput {
    Nothing,
    WholeLines,
    PartLine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramEnding {
    Exited(i32),
    Signaled(i32),
}

#[derive(Debug, PartialEq, Eq)]
pub struct TestOutcome {
    pub selected: usize,
    pub all_passed: bool,
}

impl<W: Write> TestReport<W> {
    const INDENT: &'static str = "    ";
    const NEWLINE: u8 = b'\n';

    pub fn new(output: W) -> Self {
        Self {
            output,
            counts: TestCounts::default(),
            running: None,
            finished: false,
        }
    }

    pub fn program_output(&mut self, bytes: &[u8]) -> io::Result<()> {
        if let Some(running) = &mut self.running
            && let Some(last) = bytes.last()
        {
            running.output = if *last == Self::NEWLINE {
                TestOutput::WholeLines
            } else {
                TestOutput::PartLine
            };
        }
        self.output.write_all(bytes)?;
        self.output.flush()
    }

    pub fn record(&mut self, record: TestRecord) -> io::Result<()> {
        match record {
            TestRecord::Untested(untested) => self.untested(&untested),
            TestRecord::Start(selected) => self.start(selected),
            TestRecord::Test(name) => self.begin(name),
            TestRecord::Ok => self.pass(),
            TestRecord::Fail(failure) => self.fail(&failure),
            TestRecord::Panic(panic) => self.panic(&panic),
            TestRecord::Done => self.done(),
        }
    }

    pub fn finish(&mut self, ending: ProgramEnding) -> io::Result<TestOutcome> {
        let crashed = !self.finished;
        if crashed {
            self.crash(ending)?;
        }
        Ok(TestOutcome {
            selected: self.counts.selected,
            all_passed: !crashed && self.counts.failed == 0,
        })
    }

    #[cfg(test)]
    pub fn written(&self) -> &W {
        &self.output
    }

    fn untested(&mut self, untested: &UntestedNames) -> io::Result<()> {
        let count = untested.names.split_whitespace().count();
        write!(
            self.output,
            "untested in {} ({count} of {})",
            untested.module, untested.exports
        )?;
        if count > 0 {
            write!(self.output, ": {}", untested.names)?;
        }
        writeln!(self.output)
    }

    fn start(&mut self, selected: usize) -> io::Result<()> {
        self.counts.selected = selected;
        writeln!(self.output, "running {}", Self::tests(selected))
    }

    fn begin(&mut self, name: TestName) -> io::Result<()> {
        write!(self.output, "test {} ... ", name.name)?;
        self.output.flush()?;
        self.running = Some(RunningTest {
            name,
            output: TestOutput::Nothing,
        });
        Ok(())
    }

    fn pass(&mut self) -> io::Result<()> {
        self.counts.passed += 1;
        self.end_test("ok").map(|_| ())
    }

    fn fail(&mut self, failure: &TestFailure) -> io::Result<()> {
        self.counts.failed += 1;
        let name = self.end_test("FAIL")?;
        self.failure_heading(&name, &failure.location)?;
        self.indented(&failure.written)?;
        self.failure_details(failure)
    }

    fn failure_details(&mut self, failure: &TestFailure) -> io::Result<()> {
        if let Some((left, right)) = &failure.sides {
            self.sides(left, right)?;
        }
        match &failure.message {
            Some(message) => self.indented(message),
            None => Ok(()),
        }
    }

    fn sides(&mut self, left: &str, right: &str) -> io::Result<()> {
        self.indented(format_args!("left:  {left}"))?;
        self.indented(format_args!("right: {right}"))
    }

    fn panic(&mut self, panic: &TestPanic) -> io::Result<()> {
        self.counts.failed += 1;
        let name = self.end_test("FAIL")?;
        self.failure_heading(&name, &panic.location)?;
        self.indented(&panic.message)
    }

    fn done(&mut self) -> io::Result<()> {
        self.finished = true;
        writeln!(
            self.output,
            "{} passed, {} failed",
            self.counts.passed, self.counts.failed
        )
    }

    fn crash(&mut self, ending: ProgramEnding) -> io::Result<()> {
        let crashed = self.running.is_some();
        match self.running.take() {
            Some(running) => {
                self.result_line(running.output, "CRASH")?;
                writeln!(self.output, "CRASH {}: {ending}", running.name.name)?;
            }
            None => writeln!(self.output, "CRASH: {ending}")?,
        }
        let ended = self.counts.passed + self.counts.failed + usize::from(crashed);
        let not_run = self.counts.selected.saturating_sub(ended);
        if not_run > 0 {
            writeln!(self.output, "{} not run", Self::tests(not_run))?;
        }
        writeln!(
            self.output,
            "{} passed, {} failed, {} crashed, {not_run} not run",
            self.counts.passed,
            self.counts.failed,
            usize::from(crashed)
        )
    }

    fn end_test(&mut self, result: &str) -> io::Result<TestName> {
        let running = self
            .running
            .take()
            .expect("a result record follows a test record");
        self.result_line(running.output, result)?;
        Ok(running.name)
    }

    fn result_line(&mut self, output: TestOutput, result: &str) -> io::Result<()> {
        if output == TestOutput::PartLine {
            writeln!(self.output)?;
        }
        writeln!(self.output, "{result}")
    }

    fn failure_heading(&mut self, name: &TestName, location: &str) -> io::Result<()> {
        write!(self.output, "FAIL {}", name.name)?;
        if !name.summary.is_empty() {
            write!(self.output, ": {}", name.summary)?;
        }
        writeln!(self.output, " ({location})")
    }

    fn indented(&mut self, text: impl Display) -> io::Result<()> {
        for line in text.to_string().lines() {
            writeln!(self.output, "{}{line}", Self::INDENT)?;
        }
        Ok(())
    }

    fn tests(count: usize) -> String {
        match count {
            1 => "1 test".to_string(),
            _ => format!("{count} tests"),
        }
    }
}

impl ProgramEnding {
    const SIGNAL_NAMES: [(i32, &'static str); 12] = [
        (1, "SIGHUP"),
        (2, "SIGINT"),
        (3, "SIGQUIT"),
        (4, "SIGILL"),
        (5, "SIGTRAP"),
        (6, "SIGABRT"),
        (8, "SIGFPE"),
        (9, "SIGKILL"),
        (11, "SIGSEGV"),
        (13, "SIGPIPE"),
        (14, "SIGALRM"),
        (15, "SIGTERM"),
    ];

    fn signal_name(signal: i32) -> Option<&'static str> {
        Self::SIGNAL_NAMES
            .iter()
            .find(|(number, _)| *number == signal)
            .map(|(_, name)| *name)
    }
}

impl Display for ProgramEnding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exited(code) => write!(formatter, "the program exited with code {code}"),
            Self::Signaled(signal) => {
                write!(formatter, "the program stopped on signal {signal}")?;
                match Self::signal_name(*signal) {
                    Some(name) => write!(formatter, " ({name})"),
                    None => Ok(()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ProgramEnding, TestOutcome, TestReport};
    use crate::cli::test_record::TestRecord;

    struct ReportFixture {
        report: TestReport<Vec<u8>>,
    }

    impl ReportFixture {
        const CLEAN_EXIT: ProgramEnding = ProgramEnding::Exited(0);
        const PADS_TEST: &str = "(test \"pads/left\" \"\")";
        const DOCUMENTED_TEST: &str = "(test \"pads/right\" \"Pads on the right.\")";

        fn new() -> Self {
            Self {
                report: TestReport::new(Vec::new()),
            }
        }

        fn records(mut self, texts: &[&str]) -> Self {
            for text in texts {
                let record = TestRecord::parse(text).expect("the fixture writes valid records");
                self.report.record(record).expect("a vector takes output");
            }
            self
        }

        fn output(mut self, bytes: &[u8]) -> Self {
            self.report
                .program_output(bytes)
                .expect("a vector takes output");
            self
        }

        fn finished(mut self, ending: ProgramEnding) -> (String, TestOutcome) {
            let outcome = self.report.finish(ending).expect("a vector takes output");
            let printed = String::from_utf8(self.report.output).expect("the report is text");
            (printed, outcome)
        }
    }

    #[test]
    fn a_passing_test_prints_one_line() {
        let (printed, outcome) = ReportFixture::new()
            .records(&["(start 1)", ReportFixture::PADS_TEST, "(ok)", "(done)"])
            .finished(ReportFixture::CLEAN_EXIT);
        assert_eq!(
            printed,
            "running 1 test\ntest pads/left ... ok\n1 passed, 0 failed\n"
        );
        assert_eq!(
            outcome,
            TestOutcome {
                selected: 1,
                all_passed: true
            }
        );
    }

    #[test]
    fn a_failed_equality_prints_its_summary_both_sides_and_its_message() {
        let (printed, outcome) = ReportFixture::new()
            .records(&[
                "(start 1)",
                ReportFixture::DOCUMENTED_TEST,
                "(fail-equal \"pads.rslisp:4:3\" \"(assert (= 1 2) \\\"close\\\")\" \"1\" \"2\" \"close\")",
                "(done)",
            ])
            .finished(ReportFixture::CLEAN_EXIT);
        assert_eq!(
            printed,
            "running 1 test\n\
             test pads/right ... FAIL\n\
             FAIL pads/right: Pads on the right. (pads.rslisp:4:3)\n    \
             (assert (= 1 2) \"close\")\n    \
             left:  1\n    \
             right: 2\n    \
             close\n\
             0 passed, 1 failed\n"
        );
        assert!(!outcome.all_passed);
    }

    #[test]
    fn a_failed_assert_without_sides_prints_the_written_form() {
        let (printed, _) = ReportFixture::new()
            .records(&[
                "(start 1)",
                ReportFixture::PADS_TEST,
                "(fail \"pads.rslisp:2:3\" \"(assert false)\")",
                "(done)",
            ])
            .finished(ReportFixture::CLEAN_EXIT);
        assert_eq!(
            printed,
            "running 1 test\ntest pads/left ... FAIL\nFAIL pads/left (pads.rslisp:2:3)\n    (assert false)\n0 passed, 1 failed\n"
        );
    }

    #[test]
    fn a_panic_prints_its_message_under_the_location_of_the_test() {
        let (printed, _) = ReportFixture::new()
            .records(&[
                "(start 1)",
                ReportFixture::PADS_TEST,
                "(panic \"pads.rslisp:1:1\" \"first\\nsecond\")",
                "(done)",
            ])
            .finished(ReportFixture::CLEAN_EXIT);
        assert_eq!(
            printed,
            "running 1 test\ntest pads/left ... FAIL\nFAIL pads/left (pads.rslisp:1:1)\n    first\n    second\n0 passed, 1 failed\n"
        );
    }

    #[test]
    fn output_without_a_final_newline_moves_the_result_to_the_next_line() {
        let (printed, _) = ReportFixture::new()
            .records(&["(start 1)", ReportFixture::PADS_TEST])
            .output(b"hello")
            .records(&["(ok)", "(done)"])
            .finished(ReportFixture::CLEAN_EXIT);
        assert_eq!(
            printed,
            "running 1 test\ntest pads/left ... hello\nok\n1 passed, 0 failed\n"
        );
    }

    #[test]
    fn output_that_ends_a_line_keeps_the_result_on_the_next_line() {
        let (printed, _) = ReportFixture::new()
            .records(&["(start 1)", ReportFixture::PADS_TEST])
            .output(b"hello\n")
            .records(&["(ok)", "(done)"])
            .finished(ReportFixture::CLEAN_EXIT);
        assert_eq!(
            printed,
            "running 1 test\ntest pads/left ... hello\nok\n1 passed, 0 failed\n"
        );
    }

    #[test]
    fn a_crash_names_its_test_and_counts_the_tests_not_run() {
        let (printed, outcome) = ReportFixture::new()
            .records(&[
                "(start 3)",
                "(test \"crash/first\" \"\")",
                "(ok)",
                "(test \"crash/crashes\" \"\")",
            ])
            .finished(ProgramEnding::Exited(3));
        assert_eq!(
            printed,
            "running 3 tests\n\
             test crash/first ... ok\n\
             test crash/crashes ... CRASH\n\
             CRASH crash/crashes: the program exited with code 3\n\
             1 test not run\n\
             1 passed, 0 failed, 1 crashed, 1 not run\n"
        );
        assert_eq!(
            outcome,
            TestOutcome {
                selected: 3,
                all_passed: false
            }
        );
    }

    #[test]
    fn a_crash_of_the_last_test_counts_no_test_not_run() {
        let (printed, _) = ReportFixture::new()
            .records(&["(start 1)", ReportFixture::PADS_TEST])
            .output(b"partial")
            .finished(ProgramEnding::Signaled(6));
        assert_eq!(
            printed,
            "running 1 test\n\
             test pads/left ... partial\n\
             CRASH\n\
             CRASH pads/left: the program stopped on signal 6 (SIGABRT)\n\
             0 passed, 0 failed, 1 crashed, 0 not run\n"
        );
    }

    #[test]
    fn a_crash_outside_a_test_names_no_test() {
        let (printed, outcome) = ReportFixture::new()
            .records(&["(start 2)"])
            .finished(ProgramEnding::Signaled(64));
        assert_eq!(
            printed,
            "running 2 tests\n\
             CRASH: the program stopped on signal 64\n\
             2 tests not run\n\
             0 passed, 0 failed, 0 crashed, 2 not run\n"
        );
        assert!(!outcome.all_passed);
    }

    #[test]
    fn the_untested_names_come_with_their_count() {
        let (printed, _) = ReportFixture::new()
            .records(&[
                "(untested \"prelude\" 221 \"dedup! sort!\")",
                "(start 0)",
                "(done)",
            ])
            .finished(ReportFixture::CLEAN_EXIT);
        assert_eq!(
            printed,
            "untested in prelude (2 of 221): dedup! sort!\nrunning 0 tests\n0 passed, 0 failed\n"
        );
    }

    #[test]
    fn no_untested_name_prints_the_count_alone() {
        let (printed, _) = ReportFixture::new()
            .records(&["(untested \"pads\" 3 \"\")", "(start 0)", "(done)"])
            .finished(ReportFixture::CLEAN_EXIT);
        assert_eq!(
            printed,
            "untested in pads (0 of 3)\nrunning 0 tests\n0 passed, 0 failed\n"
        );
    }
}
