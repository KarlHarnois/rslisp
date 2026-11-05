use std::io::{self, Read};
use std::path::Path;
use std::process::{self, Command, Stdio};

use super::program_stream::{ProgramStream, Streamed};
use super::standard_error::StandardError;
use super::test_record::TestRecord;
use super::test_report::{ProgramEnding, TestOutcome, TestReport};

pub struct TestRun<'a> {
    binary: &'a Path,
    arguments: Vec<String>,
}

pub enum TestRunError {
    NotStarted,
    Unwritable(io::Error),
}

impl<'a> TestRun<'a> {
    const READ_SIZE: usize = 8192;
    const UNKNOWN_EXIT_CODE: i32 = -1;

    pub fn new(binary: &'a Path, arguments: Vec<String>) -> Self {
        Self { binary, arguments }
    }

    pub fn run(
        &self,
        records: ProgramStream,
        report: &mut TestReport<impl io::Write>,
    ) -> Result<TestOutcome, TestRunError> {
        let mut child = Command::new(self.binary)
            .args(&self.arguments)
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|error| self.report_not_started(&error))?;
        let output = child.stdout.take().expect("standard output is piped");
        if let Err(error) = Self::read_records(output, records, report) {
            let _stopped = child.kill().and_then(|()| child.wait());
            return Err(TestRunError::Unwritable(error));
        }
        let status = child
            .wait()
            .map_err(|error| self.report_not_started(&error))?;
        report
            .finish(Self::ending(status))
            .map_err(TestRunError::Unwritable)
    }

    fn read_records(
        mut output: impl Read,
        mut records: ProgramStream,
        report: &mut TestReport<impl io::Write>,
    ) -> io::Result<()> {
        let mut buffer = [0; Self::READ_SIZE];
        while let Ok(read @ 1..) = output.read(&mut buffer) {
            for part in records.read(&buffer[..read]) {
                Self::report_part(part, report)?;
            }
        }
        records
            .finish()
            .map_or(Ok(()), |part| Self::report_part(part, report))
    }

    fn report_part(
        part: Streamed<Vec<u8>>,
        report: &mut TestReport<impl io::Write>,
    ) -> io::Result<()> {
        match part {
            Streamed::Output(bytes) => report.program_output(&bytes),
            Streamed::Value { text, .. } => match TestRecord::parse(&text) {
                Some(record) => report.record(record),
                None => report.program_output(text.as_bytes()),
            },
        }
    }

    fn ending(status: process::ExitStatus) -> ProgramEnding {
        match (status.code(), Self::signal(status)) {
            (Some(code), _) => ProgramEnding::Exited(code),
            (None, Some(signal)) => ProgramEnding::Signaled(signal),
            (None, None) => ProgramEnding::Exited(Self::UNKNOWN_EXIT_CODE),
        }
    }

    #[cfg(unix)]
    fn signal(status: process::ExitStatus) -> Option<i32> {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    }

    #[cfg(not(unix))]
    fn signal(_status: process::ExitStatus) -> Option<i32> {
        None
    }

    fn report_not_started(&self, error: &io::Error) -> TestRunError {
        StandardError::write_line(format_args!(
            "error: cannot run `{}`: {error}",
            self.binary.display()
        ));
        TestRunError::NotStarted
    }
}

#[cfg(test)]
mod tests {
    use super::TestRun;
    use crate::cli::program_stream::Streamed;
    use crate::cli::test_report::TestReport;

    #[test]
    fn a_marked_line_that_is_no_record_passes_through_as_output() {
        let mut report = TestReport::new(Vec::new());
        let part = Streamed::Value {
            text: "(finish 1)".to_string(),
            form: 1,
        };
        TestRun::report_part(part, &mut report).expect("a vector takes output");
        assert_eq!(report.written(), b"(finish 1)");
    }
}
