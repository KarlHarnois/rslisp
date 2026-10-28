mod callgrind;
mod comparison;
mod compiler;
mod corpus;
mod failure;
mod fixed_seed;
mod header;
mod history;
mod measurement;
mod method;
mod profile;
mod results;
mod table;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand, ValueEnum};

use crate::failure::TaskFailure;
use crate::language::LanguageSource;
use callgrind::Callgrind;
use comparison::Comparison;
use compiler::{BuiltCompiler, CompilerBuild};
use corpus::Corpus;
use fixed_seed::FixedSeedLibrary;
use history::BenchHistory;
use method::MeasurementMethod;
use profile::EntryPointCheck;
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
    #[command(
        about = "Print the key of what besides the compiler's code decides the counts: the toolchain, the benchmark code and the corpus"
    )]
    Method,
    #[command(
        about = "Write a commit's saved results from its git note, if this workspace measures by the same method"
    )]
    Recall {
        commit: String,
        #[arg(long, help = "Write the recalled results to this file for `compare`")]
        save: PathBuf,
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
            Self::Method => MeasurementMethod::new(root)
                .key()
                .map(|key| format!("{key}\n")),
            Self::Recall { commit, save } => Self::recall(root, &commit, &save),
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

    fn recall(root: &Path, commit: &str, save: &Path) -> Result<String, TaskFailure> {
        let method_key = MeasurementMethod::new(root).key()?;
        let results = BenchHistory::new(root)
            .note_of(commit)?
            .recall(&method_key)?;
        fs::write(save, results).map_err(|error| TaskFailure::unwritable(save, error))?;
        Ok(format!(
            "recalled the results of `{commit}` into `{}`\n",
            save.display()
        ))
    }
}

impl MeasureOptions {
    fn entry_point_check(&self) -> EntryPointCheck {
        match self.workspace {
            Some(_) => EntryPointCheck::Skipped,
            None => EntryPointCheck::Enforced,
        }
    }
}

struct FrontEndBenchmark<'a> {
    root: &'a Path,
    options: MeasureOptions,
}

impl<'a> FrontEndBenchmark<'a> {
    const PROFILES: &'static str = "target/bench";
    const EXTENSION_CONSTANT: &'static str = "EXTENSION";
    const FIXED_COMPILER: &'static str = "target/bench/compiler/front-end";

    fn new(root: &'a Path, options: MeasureOptions) -> Self {
        Self { root, options }
    }

    fn measure(&self) -> Result<String, TaskFailure> {
        let compiler = self.place_at_fixed_path(CompilerBuild::new(&self.workspace()?).run()?)?;
        let fixed_seed = FixedSeedLibrary::new(self.root).build()?;
        let callgrind = Callgrind::new(
            compiler,
            self.options.entry_point_check(),
            fixed_seed,
            self.root.join(Self::PROFILES),
        );
        let results = self
            .profile_corpus(&callgrind)?
            .with_header(MeasurementMethod::new(self.root).header()?);
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

    fn profile_corpus(&self, callgrind: &Callgrind) -> Result<ResultSet, TaskFailure> {
        let extension = LanguageSource::new(self.root).constant(Self::EXTENSION_CONSTANT)?;
        let programs = self.corpus(&extension)?.programs()?;
        Ok(ResultSet::new(callgrind.measure_all(&programs)?))
    }

    fn corpus(&self, extension: &str) -> Result<Corpus, TaskFailure> {
        let directory = match &self.options.corpus {
            Some(corpus) => Self::absolute(corpus)?,
            None => self.root.join(Corpus::DIRECTORY),
        };
        Ok(Corpus::new(directory, extension.to_string()))
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
