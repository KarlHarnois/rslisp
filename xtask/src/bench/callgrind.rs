use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use rslisp::language::Language;

use super::profile::ProfileReader;
use super::results::ProgramResult;
use crate::failure::TaskFailure;

pub struct Callgrind {
    compiler: PathBuf,
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
        "--cache-sim=yes",
        "--compress-strings=no",
        "--compress-pos=no",
        "--dump-instr=no",
    ];
    const FIXED_CACHE_MODEL: [&'static str; 3] =
        ["--I1=32768,8,64", "--D1=32768,8,64", "--LL=8388608,16,64"];
    const FRONT_END_COMMAND: &'static str = "emit";
    const PROFILE_EXTENSION: &'static str = "callgrind";

    pub fn new(compiler: PathBuf, fixed_seed: PathBuf, profiles: PathBuf) -> Self {
        Self {
            compiler,
            fixed_seed,
            profiles,
        }
    }

    pub fn measure(&self, program: &Path) -> Result<ProgramResult, TaskFailure> {
        let profile = self.profile_path(program);
        Self::remove_stale(&profile)?;
        let output = self.run(program, &profile)?;
        let text = fs::read_to_string(&profile)
            .map_err(|_| TaskFailure::profile_missing(program, &output.stderr))?;
        let measurement = match output.status.success() {
            true => Some(ProfileReader::new(Language::NAME).read(&text)?),
            false => None,
        };
        Ok(ProgramResult {
            program: Self::name_of(program),
            measurement,
        })
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
            .args(Self::FIXED_CACHE_MODEL)
            .arg(format!("--callgrind-out-file={}", profile.display()))
            .arg(&self.compiler)
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
