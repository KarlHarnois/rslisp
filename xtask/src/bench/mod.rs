mod callgrind;
mod comparison;
mod compiler;
mod corpus;
mod failure;
mod fixed_seed;
mod measurement;
mod profile;
mod results;
mod table;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand, ValueEnum};
use rslisp::language::Language;

use crate::failure::TaskFailure;
use callgrind::Callgrind;
use comparison::Comparison;
use compiler::{BuiltCompiler, CompilerBuild};
use corpus::Corpus;
use fixed_seed::FixedSeedLibrary;
use results::ResultSet;

#[derive(Subcommand)]
pub enum BenchTask {
    #[command(
        about = "Count the instructions the compiler's front end runs on each corpus program, under callgrind"
    )]
    Measure(MeasureOptions),
    #[command(about = "Print a Markdown table of the change between two saved results")]
    Compare {
        baseline: PathBuf,
        candidate: PathBuf,
        #[arg(long, value_enum, default_value_t = ReportLength::Full)]
        length: ReportLength,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ReportLength {
    #[value(help = "Every stage of every program")]
    Full,
    #[value(
        help = "The number of notable rises and the largest changes, short enough for a comment"
    )]
    Brief,
}

impl ReportLength {
    fn render(self, comparison: &Comparison) -> String {
        match self {
            Self::Full => comparison.full_report(),
            Self::Brief => comparison.brief_report(),
        }
    }
}

#[derive(Args)]
pub struct MeasureOptions {
    #[arg(
        long,
        help = "The checkout whose compiler to build and measure [default: this workspace]"
    )]
    workspace: Option<PathBuf>,
    #[arg(
        long,
        help = "The directory of programs to measure [default: this workspace's tests/fixtures/corpus]"
    )]
    corpus: Option<PathBuf>,
    #[arg(long, help = "Write the results to this file for `compare`")]
    save: Option<PathBuf>,
}

impl BenchTask {
    pub fn run(self, root: &Path) -> ExitCode {
        let report = match self {
            Self::Measure(options) => FrontEndBenchmark::new(root, options).measure(),
            Self::Compare {
                baseline,
                candidate,
                length,
            } => Self::compare(&baseline, &candidate, length),
        };
        match report {
            Ok(report) => {
                print!("{report}");
                ExitCode::SUCCESS
            }
            Err(failure) => failure.report(),
        }
    }

    fn compare(
        baseline: &Path,
        candidate: &Path,
        length: ReportLength,
    ) -> Result<String, TaskFailure> {
        let baseline = ResultSet::load(baseline)?;
        let candidate = ResultSet::load(candidate)?;
        Ok(length.render(&Comparison::new(&baseline, &candidate)))
    }
}

struct FrontEndBenchmark<'a> {
    root: &'a Path,
    options: MeasureOptions,
}

impl<'a> FrontEndBenchmark<'a> {
    const PROFILES: &'static str = "target/bench";
    const EMPTY_PROGRAM: &'static str = "xtask/src/bench/empty_program";
    const FIXED_COMPILER: &'static str = "target/bench/compiler/front-end";

    fn new(root: &'a Path, options: MeasureOptions) -> Self {
        Self { root, options }
    }

    fn measure(&self) -> Result<String, TaskFailure> {
        let compiler = self.place_at_fixed_path(CompilerBuild::new(&self.workspace()?).run()?)?;
        let fixed_seed = FixedSeedLibrary::new(self.root).build()?;
        let callgrind = Callgrind::new(compiler, fixed_seed, self.root.join(Self::PROFILES));
        let results = self.profile_corpus(&callgrind)?;
        if let Some(path) = &self.options.save {
            results.save(path)?;
        }
        Ok(results.to_string())
    }

    fn workspace(&self) -> Result<PathBuf, TaskFailure> {
        match &self.options.workspace {
            Some(workspace) => Self::absolute(workspace),
            None => Self::absolute(self.root),
        }
    }

    fn corpus(&self) -> Result<Corpus, TaskFailure> {
        let directory = match &self.options.corpus {
            Some(corpus) => Self::absolute(corpus)?,
            None => self.root.join(Corpus::DIRECTORY),
        };
        Ok(Corpus::new(directory))
    }

    fn profile_corpus(&self, callgrind: &Callgrind) -> Result<ResultSet, TaskFailure> {
        let mut programs = self.corpus()?.programs()?;
        programs.push(self.empty_program());
        Ok(ResultSet::new(callgrind.measure_all(&programs)?))
    }

    fn empty_program(&self) -> PathBuf {
        self.root
            .join(Self::EMPTY_PROGRAM)
            .with_extension(Language::EXTENSION)
    }

    fn place_at_fixed_path(&self, compiler: BuiltCompiler) -> Result<BuiltCompiler, TaskFailure> {
        let placed = self.root.join(Self::FIXED_COMPILER);
        let directory = placed.parent().unwrap_or(self.root);
        fs::create_dir_all(directory).map_err(|error| TaskFailure::unwritable(directory, error))?;
        fs::copy(&compiler.executable, &placed)
            .map_err(|error| TaskFailure::unwritable(&placed, error))?;
        Ok(BuiltCompiler {
            executable: placed,
            ..compiler
        })
    }

    fn absolute(path: &Path) -> Result<PathBuf, TaskFailure> {
        path.canonicalize()
            .map_err(|error| TaskFailure::unreadable(path, error))
    }
}
