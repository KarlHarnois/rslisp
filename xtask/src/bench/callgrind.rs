use std::env;
use std::fs;
use std::io::ErrorKind;
use std::num::NonZeroUsize;
use std::panic;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use super::compiler::BuiltCompiler;
use super::measurement::Measurement;
use super::profile::{EntryPointCheck, ProfileReader};
use super::results::ProgramResult;
use crate::failure::TaskFailure;

pub struct Callgrind {
    compiler: BuiltCompiler,
    entry_point_check: EntryPointCheck,
    fixed_seed: PathBuf,
    profiles: PathBuf,
}

impl Callgrind {
    const VALGRIND: &'static str = "valgrind";
    const PRELOAD: &'static str = "LD_PRELOAD";
    const SEARCH_PATH: &'static str = "PATH";
    const OPTIONS: [&'static str; 7] = [
        "--tool=callgrind",
        "--quiet",
        "--trace-children=no",
        "--cache-sim=no",
        "--compress-strings=no",
        "--compress-pos=no",
        "--dump-instr=no",
    ];
    const FRONT_END_COMMAND: &'static str = "emit";
    const PROFILE_EXTENSION: &'static str = "callgrind";

    pub fn new(
        compiler: BuiltCompiler,
        entry_point_check: EntryPointCheck,
        fixed_seed: PathBuf,
        profiles: PathBuf,
    ) -> Self {
        Self {
            compiler,
            entry_point_check,
            fixed_seed,
            profiles,
        }
    }

    pub fn measure_all(&self, programs: &[PathBuf]) -> Result<Vec<ProgramResult>, TaskFailure> {
        let next = AtomicUsize::new(0);
        let mut measured: Vec<(usize, Result<ProgramResult, TaskFailure>)> =
            thread::scope(|scope| {
                let workers: Vec<_> = (0..Self::worker_count())
                    .map(|_| scope.spawn(|| self.measure_until_done(programs, &next)))
                    .collect();
                workers
                    .into_iter()
                    .flat_map(|worker| {
                        worker
                            .join()
                            .unwrap_or_else(|panic| panic::resume_unwind(panic))
                    })
                    .collect()
            });
        measured.sort_by_key(|(index, _)| *index);
        measured.into_iter().map(|(_, result)| result).collect()
    }

    fn measure_until_done(
        &self,
        programs: &[PathBuf],
        next: &AtomicUsize,
    ) -> Vec<(usize, Result<ProgramResult, TaskFailure>)> {
        let mut measured = Vec::new();
        loop {
            let index = next.fetch_add(1, Ordering::Relaxed);
            let Some(program) = programs.get(index) else {
                return measured;
            };
            measured.push((index, self.measure(program)));
        }
    }

    fn worker_count() -> usize {
        thread::available_parallelism().map_or(1, NonZeroUsize::get)
    }

    fn measure(&self, program: &Path) -> Result<ProgramResult, TaskFailure> {
        let profile = self.profile_path(program);
        Self::remove_stale(&profile)?;
        let output = self.run(program, &profile)?;
        let measurement = match RunOutcome::of(output.status) {
            RunOutcome::Accepted => Some(self.read_profile(program, &profile, &output)?),
            RunOutcome::Rejected => None,
            RunOutcome::Crashed => {
                return Err(TaskFailure::run_crashed(
                    program,
                    output.status,
                    &output.stderr,
                ));
            }
        };
        Ok(ProgramResult {
            program: Self::name_of(program),
            measurement,
        })
    }

    fn read_profile(
        &self,
        program: &Path,
        profile: &Path,
        output: &Output,
    ) -> Result<Measurement, TaskFailure> {
        let text = fs::read_to_string(profile)
            .map_err(|_| TaskFailure::profile_missing(program, &output.stderr))?;
        ProfileReader::new(&self.compiler.crate_name, self.entry_point_check)
            .read(&text)
            .map_err(|failure| failure.while_measuring(program))
    }

    fn run(&self, program: &Path, profile: &Path) -> Result<Output, TaskFailure> {
        fs::create_dir_all(&self.profiles)
            .map_err(|error| TaskFailure::unwritable(&self.profiles, error))?;
        Command::new(Self::VALGRIND)
            .env_clear()
            .env(
                Self::SEARCH_PATH,
                env::var_os(Self::SEARCH_PATH).unwrap_or_default(),
            )
            .env(Self::PRELOAD, &self.fixed_seed)
            .args(Self::OPTIONS)
            .arg(format!("--callgrind-out-file={}", profile.display()))
            .arg(&self.compiler.executable)
            .arg(Self::FRONT_END_COMMAND)
            .arg(program)
            .current_dir(program.parent().unwrap_or(Path::new(".")))
            .stdout(Stdio::null())
            .output()
            .map_err(TaskFailure::valgrind_missing)
    }

    fn profile_path(&self, program: &Path) -> PathBuf {
        self.profiles
            .join(Self::name_of(program))
            .with_extension(Self::PROFILE_EXTENSION)
    }

    fn remove_stale(profile: &Path) -> Result<(), TaskFailure> {
        match fs::remove_file(profile) {
            Err(error) if error.kind() != ErrorKind::NotFound => {
                Err(TaskFailure::unwritable(profile, error))
            }
            _ => Ok(()),
        }
    }

    fn name_of(program: &Path) -> String {
        program
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

enum RunOutcome {
    Accepted,
    Rejected,
    Crashed,
}

impl RunOutcome {
    const PROGRAM_ERRORS_EXIT_CODE: i32 = 1;

    fn of(status: ExitStatus) -> Self {
        match status.code() {
            Some(0) => Self::Accepted,
            Some(Self::PROGRAM_ERRORS_EXIT_CODE) => Self::Rejected,
            _ => Self::Crashed,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;

    use super::*;

    struct RunOutcomeFixture;

    impl RunOutcomeFixture {
        const EXITED_WITH_SUCCESS: i32 = 0;
        const EXITED_WITH_PROGRAM_ERRORS: i32 = 256;
        const KILLED_BY_SEGMENTATION_FAULT: i32 = 11;

        fn of(raw_wait_status: i32) -> RunOutcome {
            RunOutcome::of(ExitStatus::from_raw(raw_wait_status))
        }
    }

    #[test]
    fn a_successful_run_is_accepted() {
        assert!(matches!(
            RunOutcomeFixture::of(RunOutcomeFixture::EXITED_WITH_SUCCESS),
            RunOutcome::Accepted
        ));
    }

    #[test]
    fn a_run_that_reports_program_errors_is_rejected() {
        assert!(matches!(
            RunOutcomeFixture::of(RunOutcomeFixture::EXITED_WITH_PROGRAM_ERRORS),
            RunOutcome::Rejected
        ));
    }

    #[test]
    fn a_run_killed_by_a_signal_crashed() {
        assert!(matches!(
            RunOutcomeFixture::of(RunOutcomeFixture::KILLED_BY_SEGMENTATION_FAULT),
            RunOutcome::Crashed
        ));
    }
}
