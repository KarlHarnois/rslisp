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
    profiles: PathBuf,
}

impl Callgrind {
    pub const VALGRIND: &'static str = "valgrind";
    const ALLOCATOR_TUNING: &'static str = "GLIBC_TUNABLES";
    const NO_FASTBINS: &'static str = "glibc.malloc.mxfast=0";
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
        profiles: PathBuf,
    ) -> Self {
        Self {
            compiler,
            entry_point_check,
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
            RunOutcome::Accepted => Some(self.read_profile(program, &profile, &output.stderr)?),
            RunOutcome::Rejected => {
                Self::require_profile(program, &profile, &output.stderr)?;
                None
            }
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
        stderr: &[u8],
    ) -> Result<Measurement, TaskFailure> {
        let text = fs::read_to_string(profile)
            .map_err(|_| TaskFailure::profile_missing(program, stderr))?;
        ProfileReader::new(&self.compiler.crate_names, self.entry_point_check)
            .read(&text)
            .map_err(|failure| failure.while_measuring(program))
    }

    fn require_profile(program: &Path, profile: &Path, stderr: &[u8]) -> Result<(), TaskFailure> {
        match profile.exists() {
            true => Ok(()),
            false => Err(TaskFailure::profile_missing(program, stderr)),
        }
    }

    fn run(&self, program: &Path, profile: &Path) -> Result<Output, TaskFailure> {
        fs::create_dir_all(&self.profiles)
            .map_err(|error| TaskFailure::unwritable(&self.profiles, error))?;
        self.command(program, profile)
            .output()
            .map_err(TaskFailure::valgrind_missing)
    }

    fn command(&self, program: &Path, profile: &Path) -> Command {
        let mut command = Command::new(Self::VALGRIND);
        command
            .env_clear()
            .env(
                Self::SEARCH_PATH,
                env::var_os(Self::SEARCH_PATH).unwrap_or_default(),
            )
            .env(Self::ALLOCATOR_TUNING, Self::NO_FASTBINS)
            .args(Self::OPTIONS)
            .arg(format!("--callgrind-out-file={}", profile.display()))
            .arg(&self.compiler.executable)
            .arg(Self::FRONT_END_COMMAND)
            .arg(program)
            .current_dir(program.parent().unwrap_or(Path::new(".")))
            .stdout(Stdio::null());
        command
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
    use std::collections::BTreeSet;
    use std::ffi::OsStr;
    use std::os::unix::process::ExitStatusExt;

    use super::*;

    struct CallgrindFixture;

    impl CallgrindFixture {
        const EXECUTABLE: &'static str = "/checkout/target/release/compiler";
        const PROFILES: &'static str = "/profiles";
        const PROGRAM: &'static str = "/checkout/corpus/inventory";
        const PROFILE: &'static str = "/profiles/inventory.callgrind";

        fn command() -> Command {
            Self::callgrind().command(Path::new(Self::PROGRAM), Path::new(Self::PROFILE))
        }

        fn callgrind() -> Callgrind {
            let compiler = BuiltCompiler {
                executable: PathBuf::from(Self::EXECUTABLE),
                crate_names: BTreeSet::new(),
            };
            Callgrind::new(
                compiler,
                EntryPointCheck::Enforced,
                PathBuf::from(Self::PROFILES),
            )
        }
    }

    #[test]
    fn turns_off_glibc_fastbins() {
        let command = CallgrindFixture::command();
        let tuning = command
            .get_envs()
            .find(|(variable, _)| *variable == OsStr::new(Callgrind::ALLOCATOR_TUNING))
            .and_then(|(_, value)| value);
        assert_eq!(tuning, Some(OsStr::new("glibc.malloc.mxfast=0")));
    }

    struct RunOutcomeFixture;

    impl RunOutcomeFixture {
        const EXITED_WITH_SUCCESS: i32 = 0;
        const EXITED_WITH_PROGRAM_ERRORS: i32 = 256;
        const KILLED_BY_SEGMENTATION_FAULT: i32 = 11;
        const EXITED_WITH_PANIC: i32 = 101 << 8;

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

    #[test]
    fn a_run_that_panicked_crashed() {
        assert!(matches!(
            RunOutcomeFixture::of(RunOutcomeFixture::EXITED_WITH_PANIC),
            RunOutcome::Crashed
        ));
    }
}
