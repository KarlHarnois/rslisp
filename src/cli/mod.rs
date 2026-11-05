mod build;
mod build_directory;
mod capped_report;
mod check;
mod color_when;
mod counter_report;
mod definition_buffer;
mod definition_rounds;
mod diagnostic_output;
mod doc;
mod edited_lines;
mod emit;
mod enter_key;
mod error_location;
mod eval;
mod evaluation_output;
mod exit_status;
mod expand;
mod finished_program;
mod input_helper;
mod interruption;
mod json_replies;
mod name_records;
mod open_input;
mod painted_program;
mod program_stream;
mod prompt_diagnostics;
mod prompt_history;
mod prompt_lines;
mod prompt_module;
mod repl;
mod run;
mod signatures;
mod stage_work;
mod standard_error;
mod streamed_program;
mod temporary_directory;
mod term_parser;
mod terminal;
mod token_colors;
mod value_marker;
mod whole_characters;

use std::convert::Infallible;
use std::env;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use anstream::AutoStream;
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};

use crate::build::BuildProfile;
use crate::counters::CountingAllocator;
use crate::language::Language;
use crate::source::TextPosition;
use build::BuildCommand;
use check::CheckCommand;
use color_when::ColorWhen;
use counter_report::CounterReport;
use doc::{DocCommand, DocEnvironment, DocPage};
use emit::EmitCommand;
use eval::{EvalCommand, EvaluationSettings, FormsText, ModuleText};
use evaluation_output::{EvaluationOutput, JsonEvaluation, PrintedEvaluation};
use exit_status::ExitStatus;
use expand::ExpandCommand;
use repl::ReplCommand;
use run::RunCommand;
use signatures::SignaturesCommand;
use term_parser::{SurplusParser, TermParser};
use terminal::{BuildOptions, Terminal};

#[derive(Parser)]
#[command(
    name = Language::NAME,
    version,
    about = "Compile Lisp source files to Rust",
    after_help = "With no command, start the prompt as repl does."
)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[arg(long, global = true, hide = true)]
    counters: bool,
    #[arg(
        long,
        global = true,
        value_enum,
        value_name = "WHEN",
        default_value_t,
        help = "When to color the output"
    )]
    color: ColorWhen,
}

#[derive(Subcommand)]
enum Command {
    #[command(flatten)]
    Inspection(InspectionCommand),
    #[command(flatten)]
    Program(ProgramCommand),
}

#[derive(Subcommand)]
enum InspectionCommand {
    #[command(about = "Read, expand and check a program, then print its diagnostics")]
    Check { file: PathBuf },
    #[command(about = "Print the inferred signature of each function in a program")]
    Signatures { file: PathBuf },
    #[command(
        name = DocCommand::NAME,
        about = "Print the page of a prelude term, or of a program's term with --file"
    )]
    Doc {
        #[arg(allow_hyphen_values = true, value_parser = TermParser)]
        term: String,
        #[arg(hide = true, value_parser = SurplusParser)]
        surplus: Vec<Infallible>,
        #[arg(long)]
        file: Option<PathBuf>,
        #[command(flatten)]
        types: TypesFlag,
    },
    #[command(about = "Print a program as kernel forms after macro expansion")]
    Expand { file: PathBuf },
    #[command(about = "Print the Rust code generated for a program")]
    Emit { file: PathBuf },
}

#[derive(Subcommand)]
enum ProgramCommand {
    #[command(about = "Compile a program to a native binary")]
    Build {
        file: PathBuf,
        #[command(flatten)]
        release: ReleaseFlag,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    #[command(about = "Check a module, then run forms once in its scope and print their values")]
    Eval {
        file: PathBuf,
        #[arg(allow_hyphen_values = true)]
        forms: String,
        #[command(flatten)]
        stdin: StdinFlag,
        #[arg(
            long,
            help = "Resolve `use` from this directory [default: the file's directory]"
        )]
        root: Option<PathBuf>,
        #[command(flatten)]
        position: PositionFlags,
        #[command(flatten)]
        release: ReleaseFlag,
        #[command(flatten)]
        json: JsonFlags,
    },
    #[command(about = "Read forms at a prompt, keep the definitions and run the rest")]
    Repl {
        #[arg(long, default_value = ReplCommand::DEFAULT_ROOT, help = "Resolve `use` from this directory")]
        root: PathBuf,
        #[command(flatten)]
        release: ReleaseFlag,
    },
    #[command(about = "Build a program and run it")]
    Run {
        file: PathBuf,
        #[command(flatten)]
        release: ReleaseFlag,
        #[arg(last = true)]
        args: Vec<String>,
    },
}

#[derive(Args)]
struct ReleaseFlag {
    #[arg(long)]
    release: bool,
}

#[derive(Args)]
struct StdinFlag {
    #[arg(long, help = "Read the module's text from standard input")]
    stdin: bool,
}

#[derive(Args)]
struct PositionFlags {
    #[arg(
        long,
        requires = "column",
        help = "The line where the forms start in the module's text"
    )]
    line: Option<usize>,
    #[arg(
        long,
        requires = "line",
        help = "The byte column where the forms start, from 1"
    )]
    column: Option<usize>,
}

#[derive(Args)]
struct JsonFlags {
    #[arg(
        long,
        help = "Write one JSON object per line for each output, value and diagnostic"
    )]
    json: bool,
    #[arg(
        long,
        requires = "json",
        help = "Echo this text as `id` on every JSON object"
    )]
    id: Option<String>,
}

#[derive(Args)]
struct TypesFlag {
    #[arg(long, help = "Print the inferred type above the page")]
    types: bool,
}

impl Cli {
    pub fn parse_arguments() -> Self {
        let arguments: Vec<OsString> = env::args_os().collect();
        let mut command = Self::command().color(ColorWhen::written_in(&arguments).into());
        Self::parse_with(&mut command, arguments).unwrap_or_else(|error| error.exit())
    }

    fn parse_with(
        command: &mut clap::Command,
        arguments: Vec<OsString>,
    ) -> Result<Self, clap::Error> {
        let mut matches = command.try_get_matches_from_mut(arguments)?;
        Self::from_arg_matches_mut(&mut matches).map_err(|error| error.format(command))
    }

    pub fn run(self, allocator: Option<&CountingAllocator>) -> ExitCode {
        anstream::ColorChoice::from(self.color).write_global();
        let mut terminal = Terminal::new(AutoStream::choice(&io::stderr()));
        let command = self.command.unwrap_or_else(Command::prompt);
        let status = command.execute(&mut terminal);
        if self.counters {
            let mut report = CounterReport::new(allocator);
            report.add_stage_work(terminal.work());
            report.print();
        }
        status.into()
    }
}

impl Command {
    fn prompt() -> Self {
        Self::Program(ProgramCommand::Repl {
            root: PathBuf::from(ReplCommand::DEFAULT_ROOT),
            release: ReleaseFlag { release: false },
        })
    }

    fn execute(self, terminal: &mut Terminal) -> ExitStatus {
        match self {
            Command::Inspection(command) => command.execute(terminal),
            Command::Program(command) => command.execute(terminal),
        }
    }
}

impl InspectionCommand {
    fn execute(self, terminal: &mut Terminal) -> ExitStatus {
        match self {
            Self::Check { file } => CheckCommand::new(file).run(terminal),
            Self::Signatures { file } => SignaturesCommand::new(file).run(terminal),
            Self::Doc {
                term, file, types, ..
            } => DocCommand::new(DocEnvironment::new(file), term, types.page()).run(terminal),
            Self::Expand { file } => ExpandCommand::new(file).run(terminal),
            Self::Emit { file } => EmitCommand::new(file).run(terminal),
        }
    }
}

impl ProgramCommand {
    fn execute(self, terminal: &mut Terminal) -> ExitStatus {
        match self {
            Self::Build {
                file,
                release,
                output,
            } => BuildCommand::new(
                file,
                BuildOptions {
                    profile: release.profile(),
                    output,
                },
            )
            .run(terminal),
            Self::Eval {
                file,
                forms,
                stdin,
                root,
                position,
                release,
                json,
            } => EvalCommand::new(
                stdin.module_text(file),
                root,
                FormsText::new(forms, position.position()),
                EvaluationSettings {
                    profile: release.profile(),
                    output: json.output(),
                },
            )
            .run(terminal),
            Self::Repl { root, release } => ReplCommand::new(root, release.profile()).run(terminal),
            Self::Run {
                file,
                release,
                args,
            } => RunCommand::new(
                file,
                BuildOptions {
                    profile: release.profile(),
                    output: None,
                },
                args,
            )
            .run(terminal),
        }
    }
}

impl ReleaseFlag {
    fn profile(&self) -> BuildProfile {
        if self.release {
            BuildProfile::Release
        } else {
            BuildProfile::Debug
        }
    }
}

impl StdinFlag {
    fn module_text(&self, file: PathBuf) -> ModuleText {
        if self.stdin {
            ModuleText::StandardInput(file)
        } else {
            ModuleText::File(file)
        }
    }
}

impl PositionFlags {
    fn position(&self) -> Option<TextPosition> {
        Some(TextPosition::new(self.line?, self.column?))
    }
}

impl JsonFlags {
    fn output(self) -> Box<dyn EvaluationOutput> {
        if self.json {
            Box::new(JsonEvaluation::new(self.id))
        } else {
            Box::new(PrintedEvaluation::for_standard_output())
        }
    }
}

impl TypesFlag {
    fn page(&self) -> DocPage {
        if self.types {
            DocPage::Typed
        } else {
            DocPage::Written
        }
    }
}
