mod callgrind;
mod comparison;
mod corpus;
mod cost;
mod failure;
mod fixed_seed;
mod measurement;
mod profile;
mod results;
mod table;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Subcommand;
use rslisp::language::Language;

use crate::cargo::Cargo;
use crate::failure::TaskFailure;
use callgrind::Callgrind;
use comparison::Comparison;
use corpus::Corpus;
use fixed_seed::FixedSeedLibrary;
use results::ResultSet;

#[derive(Subcommand)]
pub enum BenchTask {
    #[command(
        about = "Count the instructions the compiler's front end runs on each corpus program, under callgrind"
    )]
    Measure {
        #[arg(
            long,
            help = "The checkout whose compiler to build and measure [default: this workspace]"
        )]
        workspace: Option<PathBuf>,
        #[arg(long, default_value = Corpus::DIRECTORY)]
        corpus: PathBuf,
        #[arg(long, help = "Write the results to this file for `compare`")]
        save: Option<PathBuf>,
    },
    #[command(about = "Print a Markdown table of the change between two saved results")]
    Compare {
        baseline: PathBuf,
        candidate: PathBuf,
    },
}

impl BenchTask {
    pub fn run(self, root: &Path) -> ExitCode {
        let report = match self {
            Self::Measure {
                workspace,
                corpus,
                save,
            } => FrontEndBenchmark::new(root, corpus).measure(workspace, save),
            Self::Compare {
                baseline,
                candidate,
            } => Self::compare(&baseline, &candidate),
        };
        match report {
            Ok(report) => {
                print!("{report}");
                ExitCode::SUCCESS
            }
            Err(failure) => failure.report(),
        }
    }

    fn compare(baseline: &Path, candidate: &Path) -> Result<String, TaskFailure> {
        let baseline = ResultSet::load(baseline)?;
        let candidate = ResultSet::load(candidate)?;
        Ok(Comparison::new(&baseline, &candidate).to_string())
    }
}

struct FrontEndBenchmark<'a> {
    root: &'a Path,
    corpus: Corpus,
}

impl<'a> FrontEndBenchmark<'a> {
    const PROFILES: &'static str = "target/bench";
    const FIXED_COMPILER_DIRECTORY: &'static str = "target/bench/compiler";
    const RELEASE_BUILD: [&'static str; 4] = ["build", "--release", "--quiet", "--package"];
    const TARGET_DIRECTORY: &'static str = "target";
    const RELEASE_PROFILE: &'static str = "release";

    fn new(root: &'a Path, corpus: PathBuf) -> Self {
        Self {
            root,
            corpus: Corpus::new(root.join(corpus)),
        }
    }

    fn measure(
        &self,
        workspace: Option<PathBuf>,
        save: Option<PathBuf>,
    ) -> Result<String, TaskFailure> {
        let workspace = Self::absolute(&workspace.unwrap_or_else(|| self.root.to_path_buf()))?;
        let compiler = self.place_at_fixed_path(&Self::build_compiler(&workspace)?)?;
        let fixed_seed = FixedSeedLibrary::new(self.root).build()?;
        let callgrind = Callgrind::new(compiler, fixed_seed, self.root.join(Self::PROFILES));
        let results = self.profile_corpus(&callgrind)?;
        if let Some(path) = save {
            results.save(&path)?;
        }
        Ok(results.to_string())
    }

    fn profile_corpus(&self, callgrind: &Callgrind) -> Result<ResultSet, TaskFailure> {
        let results = self
            .corpus
            .programs()?
            .iter()
            .map(|program| callgrind.measure(program))
            .collect::<Result<_, _>>()?;
        Ok(ResultSet::new(results))
    }

    fn build_compiler(workspace: &Path) -> Result<PathBuf, TaskFailure> {
        let target = workspace.join(Self::TARGET_DIRECTORY);
        let output = Cargo::command()
            .args(Self::RELEASE_BUILD)
            .arg(Language::NAME)
            .arg("--target-dir")
            .arg(&target)
            .current_dir(workspace)
            .output()
            .map_err(|error| TaskFailure::tool_missing("cargo", error))?;
        if !output.status.success() {
            return Err(TaskFailure::build_failed("the compiler", &output.stderr));
        }
        Ok(target.join(Self::RELEASE_PROFILE).join(Language::NAME))
    }

    fn place_at_fixed_path(&self, compiler: &Path) -> Result<PathBuf, TaskFailure> {
        let directory = self.root.join(Self::FIXED_COMPILER_DIRECTORY);
        fs::create_dir_all(&directory)
            .map_err(|error| TaskFailure::unwritable(&directory, error))?;
        let placed = directory.join(Language::NAME);
        fs::copy(compiler, &placed).map_err(|error| TaskFailure::unwritable(&placed, error))?;
        Ok(placed)
    }

    fn absolute(path: &Path) -> Result<PathBuf, TaskFailure> {
        path.canonicalize()
            .map_err(|error| TaskFailure::unreadable(path, error))
    }
}
