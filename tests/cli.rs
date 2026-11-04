use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use rslisp::build::EmittedFormatting;
use rslisp::diagnostic::DiagnosticRenderer;
use rslisp::source::SourceMap;
use serde_json::{Map, Value, json};

const GREETING: &str = "(defn main ()\n  (println \"hello from the sandbox\"))\n";
const PRINTS_A: &str = "(defn main () (println \"A\"))\n";
const PRINTS_B: &str = "(defn main () (println \"B\"))\n";
const MARKED_PROGRAM: &str = "\u{feff}(defn main () (println 1))\n";
const UNWRAPPED_FAILURE: &str = "(defstruct failure :line i64)
(defn main ()
  (println (unwrap-ok (the (result i64 failure) (err (->failure 3))))))
";
const UNBOUND_NAME: &str = "(defn main ()\n  (println greeting))\n";
const ECHO: &str = "(def first-argument (rust \"std::env::args().nth(1).unwrap_or_default()\" string))\n\n(defn main ()\n  (println first-argument))\n";
const EXIT_THREE: &str =
    "(def exit (rust \"std::process::exit({0})\" (fn (i32) unit)))\n\n(defn main ()\n  (exit 3))\n";
const MAIN_ERROR: &str = "(defn main ()\n  (let-try ((text (slurp \"missing.txt\")))\n    (do (println text) (ok 0))))\n";
const RUSTC_SYNTAX_ERROR: &str = "(def doubled-sign (rust \"{0} + + 1\" (fn (i64) i64)))\n\n(defn main ()\n  (println (doubled-sign 1)))\n";
const CRASHING_RUSTC: &str =
    "#!/bin/sh\necho 'rustc crashed before reporting anything' >&2\nexit 101\n";
const SILENT_RUSTC: &str = "#!/bin/sh\nexit 0\n";
const PASSTHROUGH_RUSTFMT: &str =
    "#!/bin/sh\nwhile IFS= read -r line; do printf '%s\\n' \"$line\"; done\n";
const FAILING_RUSTFMT: &str = "#!/bin/sh\nprintf 'rustfmt failed in the sandbox\\n' >&2\nexit 1\n";
const BOUND_MODULE: &str = "(export person)\n\n(defstruct person :name string)\n";
const MILLION_LINES: &str = "(defn main ()\n  (for-each (range 0 1000000) (fn (i) (println i))))\n";
const THOUSAND_LINES: &str = "(defn main ()\n  (for-each (range 0 1000) (fn (i) (println i))))\n";
const ABORT: &str =
    "(def abort (rust \"std::process::abort()\" (fn () unit)))\n\n(defn main ()\n  (abort))\n";
const COUNT_INPUT_LINES: &str = "(defn main ()\n  (println (count (take (line-seq stdin) 2))))\n";
const STRUCTS: &str = "(defstruct person :name string :age i64)\n(defstruct ticket :id u32)\n(derive ticket debug)\n\n(defn main () (do))\n";
const BOUND_ENTRY: &str = "(use other/person person)\n\n(rust-trait same \"std::ops::Add\" i64 (with i64 person))\n\n(type matches (where ((a (same person))) (fn (a) bool)))\n(defn matches (x) true)\n\n(defn main () (println (matches 1)))\n";

const TRAIT_SHARED_VECTOR: &str = "(struct odometer (offset i64))\n\n(trait mileage\n  (first-reading (fn ((ref self) (ref (vec i64))) i64)))\n\n(impl mileage odometer\n  (first-reading (o readings) (+ (field o offset) (at readings 0))))\n\n(defn main ()\n  (println (first-reading (new odometer (offset 10)) (vec 1200))))\n";
const GEOMETRY: &str = "(export area)\n\n(defn area (width height) \"Multiplies a width by a height.\" (* width height))\n";
const GEOMETRY_ENTRY: &str = "(use geometry area)\n\n(defn main () (println (area 3 4)))\n";
const UNDOCUMENTED_ENTRY: &str = "(defn twice (x) (* 2 x))\n\n(defn main () (println (twice 2)))\n";
const SHADOWING_DOC_PAGE: &str = "(defrule doc-page ((term) 0))\n\n(defn main () (do))\n";
const THREADING_HEADER: &str = "(defrule ->)";
const THREAD_LAST_HEADER: &str = "(defrule ->>)";
const MINUS_HEADER: &str = "(defrule -)";
const UNKNOWN_FLAG_ERROR: &str = "error: unexpected argument '--typo' found";
const ARITHMETIC: &str = include_str!("fixtures/emit/arithmetic.rslisp");
const EMPTY_MAIN: &str = "(def main (fn () (do)))\n";
const ONE_BRANCH: &str = "(def main (fn () (if true (do) (do))))\n";

const STAGE_WORK_KEYS: [&str; 4] = [
    "expand.lowered_forms",
    "check.inferred_expressions",
    "emit.emitted_expressions",
    "build.scanned_tokens",
];
const TYPED_PROGRAM: &str = "(defn twice (x) \"Doubles a number.\" (* 2 x))\n(defn same (x) x)\n(defstruct point :x i64 :y i64)\n(def limit 10)\n(defn area (:width i64 :height i64) (* width height))\n(defn main () (println (twice limit)))\n";
const DOCUMENTED_STRUCT_BESIDE_A_FUNCTION: &str = "(defstruct point \"A point on a grid.\" :x i64 :y i64)\n(defn point () (->point 1 2))\n(defn main () (do))\n";
const DOCUMENTED_FUNCTION_BESIDE_A_STRUCT: &str = "(defstruct point :x i64 :y i64)\n(defn point () \"Makes the origin.\" (->point 0 0))\n(defn main () (do))\n";
const UNDOCUMENTED_DEF_BESIDE_A_STRUCT: &str =
    "(defstruct point :x i64 :y i64)\n(def point 3)\n(defn main () (do))\n";
const UNDOCUMENTED_FUNCTION_BESIDE_A_STRUCT: &str =
    "(defstruct point :x i64 :y i64)\n(defn point () (->point 1 2))\n(defn main () (do))\n";
const OTHER_DEFINITION_FAILS: &str = "(defn twice (x) \"Doubles a number.\" (* 2 x))\n(defn bad () (+ 1 \"a\"))\n(defn main () (println (twice 2)))\n";
const OWN_DEFINITION_FAILS: &str =
    "(defn twice (x) \"Doubles a number.\" (+ x \"a\"))\n(defn main () (do))\n";
const PAGE_PROGRAM: &str = "(defn twice (x) \"Doubles a number.\" (* 2 x))\n(def page (doc-page twice))\n(defn main () (println page))\n";
const SHADOWING_SUM: &str =
    "(defn sum (left right) (+ left right))\n(defn main () (println (sum 1 2)))\n";
const NUMBERS: &str = "(export twice)\n\n(defn twice (x) \"Doubles a number.\" (* 2 x))\n";
const NUMBERS_ENTRY: &str = "(use lib/numbers twice)\n\n(defn main () (println (twice 21)))\n";
const TWICE_TYPE: &str = "(type twice (fn (i64) i64))";
const TWICE_PAGE: &str = "(defn twice (x))\n\nDoubles a number.\n";
const POINT_TYPE: &str = "(type point (fn () point))";

const SESSION: &str = "(defn square (n) (* n n))\n(defstruct point :x i64 :y i64)\n(defn main () (println \"written main\"))\n";
const EVALUATED_GEOMETRY: &str = "(defn area (w h) (* w h))\n(area 2 3)\n(defn report ()\n  (println (area 4 5)))\n(do (defn twice (n) (* 2 n)))\n";
const GEOMETRY_DEFINITIONS: &str = "(defn area (w h) (* w h))\n(defn report ()\n  (println (area 4 5)))\n(do (defn twice (n) (* 2 n)))\n";
const AREA_OF_A_STRING: &str = "(defn area (w h) (* w h))\n(defn report () (area 2 \"3\"))\n";
const TOP_LEVEL_AREA_OF_A_STRING: &str = "(defn area (w h) (* w h))\n(area 2 \"3\")\n";
const EXITS: &str = "(def exit (rust \"std::process::exit({0})\" (fn (i32) unit)))\n(def abort (rust \"std::process::abort()\" (fn () unit)))\n";
const TOP_LEVEL_DO_OF_EXPRESSIONS: &str = "(defn square (n) (* n n))\n(do (prn (square 3)))\n";
const UNREACHABLE_ARM_IN_A_BODY: &str =
    "(defn square (n) (* n n))\n(defn pick ()\n  (match 3 (_ 1) (_ 2)))\n";
const MIXED_TOP_LEVEL_DO: &str =
    "(defn square (n) (* n n))\n(do (defn cube (n) (* n (square n))) (prn 7))\n";
const UNREACHABLE_ARM: &str = "(defn pick (x) (match x (_ 1) (_ 2)))\n";
const SHAPES: &str = "(export circle)\n(defn circle (r) (* 3 r r))\n";
const SHAPES_USER: &str = "(use shapes circle)\n";
const SHAPES_OF_TWO: &str =
    "(export circle square)\n(defn circle (r) (* 3 r r))\n(defn square (s) (* s s))\n";
const POINTS: &str = "(defstruct point :x i64 :y i64)\n";
const PROMPT_DEFINITIONS: &str = "(defn square (n) (* n n))\n(defstruct point :x i64 :y i64)\n(->point (square 2) 3)\n(println \"side \" (square 3))\n";
const PROMPT_DOC: &str =
    "(defn twice (x) \"Doubles a number.\" (* 2 x))\n(doc twice)\n(doc-page twice)\n";
const PROMPT_REDEFINITION: &str = "(defn square (n) (* n n))\n(defn cube (n) (* n (square n)))\n(cube 3)\n(defn square (n) (+ n n))\n(cube 3)\n";
const PROMPT_DROPPING_REPLACEMENT: &str =
    "(defstruct point :x i64 :y i64)\n(defn ->point (x) x)\n(->point 1 2)\n";
const PROMPT_REPLACEMENT_OF_TWO: &str = "(defn circle (r) r)\n(defn square (s) s)\n(do (defn circle (r) (+ r 1)) (defn square (s) (+ s 2)))\n(+ (circle 1) (square 1))\n";
const PROMPT_FAILING_DEFINITION: &str =
    "(defn twice (x) (* 2 x))\n(defn twice (x) (+ x \"a\"))\n(twice 4)\n";
const PROMPT_FORMS_ON_ONE_LINE: &str = "(defn twice (x) (* 2 x)) (twice 4) (twice 5)\n";
const PROMPT_MULTI_LINE: &str =
    "(defn greet (name)\n  (println \"hello, \"\n    name))\n(greet \"line\nbreak\")\n";
const PROMPT_USES: &str =
    "(use shapes circle square)\n(circle 2)\n(use shapes circle)\n(square 3)\n";
const PROMPT_PANIC: &str = "(defn twice (x) (* 2 x))\n(at (vec 1 2) 5)\n(twice 4)\n";
const PROMPT_EXIT: &str =
    "(def exit (rust \"std::process::exit({0})\" (fn (i32) unit)))\n(exit 3)\n(exit 0)\n";
const PROMPT_UNCLOSED_END: &str = "(+ 1 2)\n(defn twice (x)\n  (* 2 x)\n";
const PROMPT_NOT_UTF8: &[u8] = b"(+ 1 1)\n\xff\n";
const PROMPT_EXIT_ON_A_TERMINAL: &str =
    "(def exit (rust \"std::process::exit({0})\" (fn (i32) unit)))\n(exit 3)\n\u{4}";
const PROMPT_EXTRA_CLOSE: &str = "(+ 1 2))\n(+ 3 4)\n(+ 3 \"a\")\n";
const PROMPT_ENUM_REDEFINITION: &str = "(enum shape circle square)\n(enum shape circle square triangle)\n(match shape/triangle (triangle 3) (_ 0))\n";
const PROMPT_WARNING: &str = "(defn pick (x) (match x (_ 1) (_ 2)))\n(pick 1)\n(defn other () 1)\n(defn another () 2)\n(pick 2)\n";

const SHADOWED_PRINTLN: &str = "(defn println (x) x)\n(defn square (n) (* n n))\n";
const COUNT_THEN_FAIL: &str = "(def exit (rust \"std::process::exit({0})\" (fn (i32) unit)))\n(type count-up (fn (i64) unit))\n(defn count-up (i)\n  (println i)\n  (if (< i 1000000) (recur (+ i 1)) (exit 7)))\n";

const UNBOUND_NAMES_PAST_THE_CAP: usize = 25;

struct Sandbox {
    directory: PathBuf,
}

impl Sandbox {
    const COMPILER: &str = env!("CARGO_BIN_EXE_rslisp");

    fn new(test_name: &str) -> Self {
        let directory =
            std::env::temp_dir().join(format!("rslisp-cli-{}-{test_name}", std::process::id()));
        fs::create_dir_all(&directory).expect("the sandbox directory is writable");
        Self { directory }
    }

    const EXECUTABLE: u32 = 0o755;
    const TERMINAL_RECORDER: &str = "script";
    const COLOR_VARIABLES: [&str; 5] = ["NO_COLOR", "CLICOLOR_FORCE", "CLICOLOR", "TERM", "CI"];
    const TEMPORARY_DIRECTORY_VARIABLE: &str = "TMPDIR";
    #[cfg(target_os = "linux")]
    const FULL_DEVICE: &str = "/dev/full";

    fn with_file(self, name: &str, source: &str) -> Self {
        let path = self.directory.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("the sandbox accepts directories");
        }
        fs::write(path, source).expect("the sandbox accepts files");
        self
    }

    fn with_unbound_names(self, name: &str, count: usize) -> Self {
        let body: String = (0..count)
            .map(|index| format!("    missing{index}\n"))
            .collect();
        self.with_file(name, &format!("(defn main ()\n  (do\n{body}    1))\n"))
    }

    fn with_executable(self, name: &str, script: &str) -> Self {
        let sandbox = self.with_file(name, script);
        let permissions = fs::Permissions::from_mode(Self::EXECUTABLE);
        fs::set_permissions(sandbox.directory.join(name), permissions)
            .expect("the sandbox marks files executable");
        sandbox
    }

    fn compiler(&self, arguments: &[&str]) -> Outcome {
        Outcome::of(&mut self.command(Path::new(Self::COMPILER), arguments))
    }

    fn typed_page(&self, arguments: &[&str]) -> String {
        self.typed_lookup(arguments).assert_exit_code(0).stdout()
    }

    fn typed_lookup(&self, arguments: &[&str]) -> Outcome {
        let typed: Vec<&str> = ["doc", "--types"]
            .iter()
            .chain(arguments)
            .copied()
            .collect();
        self.compiler(&typed)
    }

    fn compiler_reading(&self, input: impl AsRef<[u8]>, arguments: &[&str]) -> Outcome {
        Self::reading(
            &mut self.command(Path::new(Self::COMPILER), arguments),
            input,
        )
    }

    fn compiler_on_a_terminal_reading(
        &self,
        input: &str,
        environment: &[(&str, &str)],
        arguments: &[&str],
    ) -> Outcome {
        let recorded = Self::recorded_arguments(arguments);
        let recorded: Vec<&str> = recorded.iter().map(String::as_str).collect();
        let mut command =
            self.color_command(Path::new(Self::TERMINAL_RECORDER), environment, &recorded);
        Self::reading_until_the_end(&mut command, input)
    }

    fn reading_until_the_end(command: &mut Command, input: &str) -> Outcome {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the terminal recorder starts");
        let mut held_input = child.stdin.take().expect("stdin is piped");
        std::io::Write::write_all(&mut held_input, input.as_bytes())
            .expect("the terminal recorder reads its input");
        let mut stdout = child.stdout.take().expect("stdout is piped");
        let mut stderr = child.stderr.take().expect("stderr is piped");
        let (stdout, stderr, status) = std::thread::scope(|scope| {
            let stdout = scope.spawn(move || Self::read_all(&mut stdout));
            let stderr = scope.spawn(move || Self::read_all(&mut stderr));
            let status = child.wait().expect("the terminal recorder finishes");
            (
                stdout.join().expect("stdout is read"),
                stderr.join().expect("stderr is read"),
                status,
            )
        });
        drop(held_input);
        Outcome {
            output: Output {
                status,
                stdout,
                stderr,
            },
        }
    }

    fn read_all(stream: &mut impl std::io::Read) -> Vec<u8> {
        let mut bytes = Vec::new();
        stream
            .read_to_end(&mut bytes)
            .expect("the stream is readable");
        bytes
    }

    fn reading(command: &mut Command, input: impl AsRef<[u8]>) -> Outcome {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the compiler starts");
        std::io::Write::write_all(
            &mut child.stdin.take().expect("stdin is piped"),
            input.as_ref(),
        )
        .expect("the compiler reads its input");
        Outcome {
            output: child.wait_with_output().expect("the compiler finishes"),
        }
    }

    fn compiler_writing_to_a_closed_pipe(&self, arguments: &[&str]) -> Outcome {
        let (reader, writer) = std::io::pipe().expect("the test opens a pipe");
        drop(reader);
        self.compiler_writing_to(Stdio::from(writer), arguments)
    }

    #[cfg(target_os = "linux")]
    fn compiler_writing_to_a_full_device(&self, arguments: &[&str]) -> Outcome {
        let full_device = fs::OpenOptions::new()
            .write(true)
            .open(Self::FULL_DEVICE)
            .expect("the full device opens");
        self.compiler_writing_to(Stdio::from(full_device), arguments)
    }

    fn compiler_writing_to(&self, standard_output: Stdio, arguments: &[&str]) -> Outcome {
        let mut command = self.command(Path::new(Self::COMPILER), arguments);
        command.stdout(standard_output);
        Outcome::of(&mut command)
    }

    fn compiler_in(&self, working_directory: &str, arguments: &[&str]) -> Outcome {
        let mut command = self.command(Path::new(Self::COMPILER), arguments);
        command.current_dir(self.directory.join(working_directory));
        Outcome::of(&mut command)
    }

    fn compiler_with_path(&self, path_directory: &str, arguments: &[&str]) -> Outcome {
        let mut command = self.command(Path::new(Self::COMPILER), arguments);
        command.env("PATH", self.directory.join(path_directory));
        Outcome::of(&mut command)
    }

    fn compiler_with_temporary_directory(&self, directory: &str, arguments: &[&str]) -> Outcome {
        let mut command = self.command(Path::new(Self::COMPILER), arguments);
        command.env(
            Self::TEMPORARY_DIRECTORY_VARIABLE,
            self.directory.join(directory),
        );
        Outcome::of(&mut command)
    }

    fn compiler_with_rustfmt_time_limit(
        &self,
        time_limit: Duration,
        arguments: &[&str],
    ) -> Outcome {
        self.compiler_with_rustfmt_time_limit_value(&time_limit.as_millis().to_string(), arguments)
    }

    fn compiler_with_rustfmt_time_limit_value(&self, value: &str, arguments: &[&str]) -> Outcome {
        let mut command = self.command(Path::new(Self::COMPILER), arguments);
        command.env(EmittedFormatting::time_limit_variable(), value);
        Outcome::of(&mut command)
    }

    fn built_program(&self, relative_path: impl AsRef<Path>) -> Outcome {
        Outcome::of(&mut self.command(&self.directory.join(relative_path), &[]))
    }

    fn outside_build_path(&self, working_directory: &str, built_path: &str) -> PathBuf {
        let sandbox = fs::canonicalize(&self.directory).expect("the sandbox directory resolves");
        let sandbox_without_root: PathBuf = sandbox
            .components()
            .filter(|component| matches!(component, Component::Normal(_)))
            .collect();
        Path::new(working_directory)
            .join("target/rslisp/outside")
            .join(sandbox_without_root)
            .join(built_path)
    }

    fn built_program_read_for_one_line(&self, relative_path: &str) -> Outcome {
        let mut child = self
            .command(&self.directory.join(relative_path), &[])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the built program starts");
        let mut first_line = String::new();
        BufReader::new(child.stdout.take().expect("stdout is piped"))
            .read_line(&mut first_line)
            .expect("the program prints a line");
        let output = child
            .wait_with_output()
            .expect("the program finishes after its reader closes");
        Outcome {
            output: Output {
                stdout: first_line.into_bytes(),
                ..output
            },
        }
    }

    fn built_program_reading_its_directory(&self, relative_path: &str) -> Outcome {
        let directory = fs::File::open(&self.directory).expect("the sandbox directory opens");
        Outcome::of(
            self.command(&self.directory.join(relative_path), &[])
                .stdin(Stdio::from(directory)),
        )
    }

    fn compiler_with_colors(&self, environment: &[(&str, &str)], arguments: &[&str]) -> Outcome {
        Outcome::of(&mut self.color_command(Path::new(Self::COMPILER), environment, arguments))
    }

    fn compiler_on_a_terminal(&self, environment: &[(&str, &str)], arguments: &[&str]) -> Outcome {
        let recorded = Self::recorded_arguments(arguments);
        let recorded: Vec<&str> = recorded.iter().map(String::as_str).collect();
        Outcome::of(&mut self.color_command(
            Path::new(Self::TERMINAL_RECORDER),
            environment,
            &recorded,
        ))
    }

    #[cfg(target_os = "linux")]
    fn recorded_arguments(arguments: &[&str]) -> Vec<String> {
        let command_line = Self::compiler_command_line(arguments);
        vec!["-qec".to_string(), command_line, "/dev/null".to_string()]
    }

    #[cfg(target_os = "macos")]
    fn recorded_arguments(arguments: &[&str]) -> Vec<String> {
        let command_line = Self::compiler_command_line(arguments);
        ["-q", "/dev/null", "sh", "-c", &command_line]
            .iter()
            .map(|argument| argument.to_string())
            .collect()
    }

    fn compiler_command_line(arguments: &[&str]) -> String {
        let words: Vec<&str> = [Self::COMPILER].iter().chain(arguments).copied().collect();
        words.join(" ")
    }

    fn written_file(&self, name: &str) -> String {
        fs::read_to_string(self.directory.join(name)).expect("the command wrote the file")
    }

    fn color_command(
        &self,
        program: &Path,
        environment: &[(&str, &str)],
        arguments: &[&str],
    ) -> Command {
        let mut command = self.command(program, arguments);
        for variable in Self::COLOR_VARIABLES {
            command.env_remove(variable);
        }
        command.envs(environment.iter().copied());
        command
    }

    fn command(&self, program: &Path, arguments: &[&str]) -> Command {
        let mut command = Command::new(program);
        command.args(arguments).current_dir(&self.directory);
        command
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).ok();
    }
}

struct Outcome {
    output: Output,
}

impl Outcome {
    const SIGPIPE: i32 = 13;
    const ESCAPE: &str = "\u{1b}[";

    fn of(command: &mut Command) -> Self {
        let output = command
            .output()
            .unwrap_or_else(|error| panic!("cannot start {command:?}: {error}"));
        Self { output }
    }

    fn assert_exit_code(&self, expected: i32) -> &Self {
        assert_eq!(
            self.output.status.code(),
            Some(expected),
            "stdout:\n{}\nstderr:\n{}",
            self.stdout(),
            self.stderr()
        );
        self
    }

    fn assert_quiet_stop(&self) -> &Self {
        let status = self.output.status;
        assert!(
            status.code() == Some(0) || status.signal() == Some(Self::SIGPIPE),
            "status: {status}\nstderr:\n{}",
            self.stderr()
        );
        assert!(!self.stderr().contains("panicked"), "{}", self.stderr());
        self
    }

    fn assert_silent_success(&self) -> &Self {
        self.assert_exit_code(0);
        assert_eq!(self.stderr(), "");
        self
    }

    fn transcript(&self) -> String {
        format!(
            "exit code: {:?}\n--- stdout\n{}--- stderr\n{}",
            self.output.status.code(),
            self.stdout(),
            self.stderr()
        )
    }

    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.output.stdout).into_owned()
    }

    fn stdout_is_colored(&self) -> bool {
        self.stdout().contains(Self::ESCAPE)
    }

    fn stderr_is_colored(&self) -> bool {
        self.stderr().contains(Self::ESCAPE)
    }

    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.output.stderr).into_owned()
    }

    fn counter_report(&self) -> Map<String, Value> {
        let stderr = self.stderr();
        let last_line = stderr
            .lines()
            .last()
            .expect("the counters follow the command");
        serde_json::from_str(last_line).expect("the last line is one JSON object")
    }

    fn json_replies(&self) -> Vec<Value> {
        self.stdout()
            .lines()
            .map(|line| serde_json::from_str(line).expect("each line is one JSON object"))
            .collect()
    }

    fn stage_work(&self) -> Map<String, Value> {
        self.counter_report()
            .into_iter()
            .filter(|(key, _)| !key.starts_with("allocator."))
            .collect()
    }

    fn work_beyond(&self, baseline: &Outcome) -> Value {
        let baseline = baseline.stage_work();
        self.stage_work()
            .into_iter()
            .map(|(key, count)| {
                let added = Self::count_of(&count) - Self::count_of(&baseline[key.as_str()]);
                (key, Value::from(added))
            })
            .collect()
    }

    fn count_of(count: &Value) -> i64 {
        count.as_i64().expect("each work count is a whole number")
    }
}

#[test]
fn help_lists_every_command() {
    let sandbox = Sandbox::new("help");
    let help = sandbox.compiler(&["--help"]);
    insta::assert_snapshot!(help.assert_exit_code(0).stdout());
}

#[test]
fn counters_flag_prints_one_json_object_after_the_command() {
    let sandbox = Sandbox::new("counters").with_file("arithmetic.rslisp", ARITHMETIC);
    let plain = sandbox.compiler(&["emit", "arithmetic.rslisp"]);
    let counted = sandbox.compiler(&["--counters", "emit", "arithmetic.rslisp"]);
    assert_eq!(counted.assert_exit_code(0).stdout(), plain.stdout());
    let stderr = counted.stderr();
    let last_line = stderr
        .lines()
        .last()
        .expect("the counters follow the command");
    let report: serde_json::Value = serde_json::from_str(last_line).expect("the last line is JSON");
    assert!(report.is_object(), "{last_line}");
    #[cfg(not(feature = "counters"))]
    assert!(
        report
            .as_object()
            .is_some_and(|counters| !counters.keys().any(|key| key.starts_with("allocator."))),
        "{last_line}"
    );
}

#[test]
fn counters_flag_stays_out_of_the_help() {
    let sandbox = Sandbox::new("counters-help");
    let help = sandbox.compiler(&["--help"]);
    assert!(!help.assert_exit_code(0).stdout().contains("--counters"));
}

#[test]
fn counters_flag_counts_a_program_small_enough_to_count_by_hand() {
    let sandbox = Sandbox::new("counters-by-hand")
        .with_file("empty.rslisp", EMPTY_MAIN)
        .with_file("branch.rslisp", ONE_BRANCH);
    let empty = sandbox.compiler(&["--counters", "emit", "empty.rslisp"]);
    let branch = sandbox.compiler(&["--counters", "emit", "branch.rslisp"]);
    assert_eq!(
        branch
            .assert_exit_code(0)
            .work_beyond(empty.assert_exit_code(0)),
        json!({
            "expand.lowered_forms": 3,
            "check.inferred_expressions": 3,
            "emit.emitted_expressions": 2,
            "build.scanned_tokens": 8,
        })
    );
}

#[test]
fn counters_flag_prints_no_build_work_when_rustfmt_fails() {
    let sandbox = Sandbox::new("counters-unformatted")
        .with_file("branch.rslisp", ONE_BRANCH)
        .with_executable("tools/rustfmt", FAILING_RUSTFMT);
    let emit = sandbox.compiler_with_path("tools", &["--counters", "emit", "branch.rslisp"]);
    let work = emit.assert_exit_code(0).stage_work();
    assert_eq!(
        work.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "check.inferred_expressions",
            "emit.emitted_expressions",
            "expand.lowered_forms"
        ]
    );
}

#[test]
fn counters_flag_prints_the_work_of_each_stage_that_ran() {
    let sandbox = Sandbox::new("counters-stages").with_file("arithmetic.rslisp", ARITHMETIC);
    let emit = ["--counters", "emit", "arithmetic.rslisp"];
    let emitted = sandbox.compiler(&emit).assert_exit_code(0).stage_work();
    for key in STAGE_WORK_KEYS {
        assert!(
            emitted[key].as_u64().is_some_and(|count| count > 0),
            "{key} in {emitted:?}"
        );
    }
    let checked = sandbox
        .compiler(&["--counters", "check", "arithmetic.rslisp"])
        .assert_exit_code(0)
        .stage_work();
    assert_eq!(
        checked.keys().map(String::as_str).collect::<Vec<_>>(),
        ["check.inferred_expressions", "expand.lowered_forms"]
    );
    assert_eq!(sandbox.compiler(&emit).stage_work(), emitted);
}

#[test]
fn unknown_command_is_a_usage_error() {
    let sandbox = Sandbox::new("unknown_command");
    sandbox.compiler(&["compile"]).assert_exit_code(2);
}

#[test]
fn unreadable_file_is_a_usage_error() {
    let sandbox = Sandbox::new("unreadable_file");
    let check = sandbox.compiler(&["check", "missing.rslisp"]);
    assert!(
        check
            .assert_exit_code(2)
            .stderr()
            .starts_with("error: cannot read `missing.rslisp`")
    );
}

#[test]
fn check_is_silent_on_a_valid_program() {
    let sandbox = Sandbox::new("check_valid").with_file("greeting.rslisp", GREETING);
    let check = sandbox.compiler(&["check", "greeting.rslisp"]);
    check.assert_exit_code(0);
    assert_eq!(
        (check.stdout(), check.stderr()),
        (String::new(), String::new())
    );
}

#[test]
fn check_ignores_a_byte_order_mark() {
    let sandbox = Sandbox::new("check_byte_order_mark").with_file("marked.rslisp", MARKED_PROGRAM);
    sandbox
        .compiler(&["check", "marked.rslisp"])
        .assert_exit_code(0);
}

#[test]
fn signatures_prints_each_function() {
    let sandbox = Sandbox::new("signatures").with_file("greeting.rslisp", GREETING);
    let signatures = sandbox.compiler(&["signatures", "greeting.rslisp"]);
    insta::assert_snapshot!(signatures.assert_exit_code(0).stdout());
}

#[test]
fn signatures_prints_inferred_struct_derives() {
    let sandbox = Sandbox::new("signatures_structs").with_file("structs.rslisp", STRUCTS);
    let signatures = sandbox.compiler(&["signatures", "structs.rslisp"]);
    insta::assert_snapshot!(signatures.assert_exit_code(0).stdout());
}

#[test]
fn doc_ends_quietly_when_its_reader_closes() {
    let sandbox = Sandbox::new("doc_closed_pipe");
    sandbox
        .compiler_writing_to_a_closed_pipe(&["doc", "if"])
        .assert_silent_success();
}

#[test]
#[cfg(target_os = "linux")]
fn doc_reports_a_standard_output_it_cannot_write() {
    let sandbox = Sandbox::new("doc_unwritable");
    let doc = sandbox.compiler_writing_to_a_full_device(&["doc", "if"]);
    assert!(
        doc.assert_exit_code(2)
            .stderr()
            .starts_with("error: cannot write standard output: "),
        "{}",
        doc.stderr()
    );
}

#[test]
fn emit_ends_quietly_when_its_reader_closes() {
    let sandbox = Sandbox::new("emit_closed_pipe").with_file("greeting.rslisp", GREETING);
    sandbox
        .compiler_writing_to_a_closed_pipe(&["emit", "greeting.rslisp"])
        .assert_silent_success();
}

#[test]
fn expand_ends_quietly_when_its_reader_closes() {
    let sandbox = Sandbox::new("expand_closed_pipe").with_file("greeting.rslisp", GREETING);
    sandbox
        .compiler_writing_to_a_closed_pipe(&["expand", "greeting.rslisp"])
        .assert_silent_success();
}

#[test]
fn signatures_ends_quietly_when_its_reader_closes() {
    let sandbox = Sandbox::new("signatures_closed_pipe").with_file("greeting.rslisp", GREETING);
    sandbox
        .compiler_writing_to_a_closed_pipe(&["signatures", "greeting.rslisp"])
        .assert_silent_success();
}

#[test]
fn doc_prints_the_page_of_a_prelude_term() {
    let sandbox = Sandbox::new("doc_prelude");
    let doc = sandbox.compiler(&["doc", "defn"]);
    assert_eq!(
        doc.assert_exit_code(0).stdout(),
        "(defmacro defn (name parameters body ...))\n\nDefines a function: (defn name (parameter ...) body ...).\n\nA string before at least one more body form is the docstring.\n"
    );
}

#[test]
fn doc_takes_a_term_that_starts_with_a_hyphen() {
    let sandbox = Sandbox::new("doc_hyphen");
    let doc = sandbox.compiler(&["doc", "->"]);
    let page = doc.assert_exit_code(0).stdout();
    assert_eq!(page.lines().next(), Some(THREADING_HEADER));
}

#[test]
fn doc_takes_the_thread_last_term() {
    let sandbox = Sandbox::new("doc_thread_last");
    let doc = sandbox.compiler(&["doc", "->>"]);
    let page = doc.assert_exit_code(0).stdout();
    assert_eq!(page.lines().next(), Some(THREAD_LAST_HEADER));
}

#[test]
fn doc_takes_the_minus_term() {
    let sandbox = Sandbox::new("doc_minus");
    let doc = sandbox.compiler(&["doc", "-"]);
    let page = doc.assert_exit_code(0).stdout();
    assert_eq!(page.lines().next(), Some(MINUS_HEADER));
}

#[test]
fn doc_reports_an_unknown_flag_before_the_term() {
    let sandbox = Sandbox::new("doc_unknown_flag_before_term");
    let doc = sandbox.compiler(&["doc", "--typo", "twice"]);
    let stderr = doc.assert_exit_code(2).stderr();
    assert_eq!(stderr.lines().next(), Some(UNKNOWN_FLAG_ERROR));
}

#[test]
fn doc_reports_an_unknown_flag_in_place_of_the_term() {
    let sandbox = Sandbox::new("doc_unknown_flag_as_term");
    let doc = sandbox.compiler(&["doc", "--typo"]);
    let stderr = doc.assert_exit_code(2).stderr();
    assert_eq!(stderr.lines().next(), Some(UNKNOWN_FLAG_ERROR));
}

#[test]
fn doc_reports_a_second_term() {
    let sandbox = Sandbox::new("doc_second_term");
    let doc = sandbox.compiler(&["doc", "defn", "defmacro"]);
    let stderr = doc.assert_exit_code(2).stderr();
    assert_eq!(
        stderr.lines().next(),
        Some("error: unexpected argument 'defmacro' found")
    );
}

#[test]
fn doc_prints_the_page_of_an_imported_term() {
    let sandbox = Sandbox::new("doc_imported")
        .with_file("geometry.rslisp", GEOMETRY)
        .with_file("main.rslisp", GEOMETRY_ENTRY);
    let doc = sandbox.compiler(&["doc", "--file", "main.rslisp", "area"]);
    assert_eq!(
        doc.assert_exit_code(0).stdout(),
        "(defn area (width height))\n\nMultiplies a width by a height.\n"
    );
}

#[test]
fn doc_prints_the_page_of_an_undocumented_term() {
    let sandbox = Sandbox::new("doc_undocumented").with_file("main.rslisp", UNDOCUMENTED_ENTRY);
    let doc = sandbox.compiler(&["doc", "--file", "main.rslisp", "twice"]);
    assert_eq!(
        doc.assert_exit_code(0).stdout(),
        "(defn twice (x))\n\nNo documentation.\n"
    );
}

#[test]
fn doc_suggests_a_close_name_for_an_unknown_term() {
    let sandbox = Sandbox::new("doc_unknown");
    let doc = sandbox.compiler(&["doc", "defm"]);
    insta::assert_snapshot!(doc.assert_exit_code(1).stderr());
}

#[test]
fn doc_rejects_a_term_that_is_not_one_name() {
    let sandbox = Sandbox::new("doc_not_one_name");
    let doc = sandbox.compiler(&["doc", "x y"]);
    assert_eq!(
        doc.assert_exit_code(2).stderr(),
        "error: `x y` is not one name\n"
    );
}

#[test]
fn doc_rejects_a_term_that_does_not_read() {
    let sandbox = Sandbox::new("doc_unreadable_term");
    let doc = sandbox.compiler(&["doc", "("]);
    assert!(doc.assert_exit_code(2).stderr().starts_with("error[L0101]"));
}

#[test]
fn doc_reports_an_unreadable_file_as_a_usage_error() {
    let sandbox = Sandbox::new("doc_unreadable_file");
    let doc = sandbox.compiler(&["doc", "--file", "missing.rslisp", "defn"]);
    assert!(
        doc.assert_exit_code(2)
            .stderr()
            .starts_with("error: cannot read `missing.rslisp`")
    );
}

#[test]
fn doc_rejects_a_term_that_is_not_one_name_before_reading_the_file() {
    let sandbox = Sandbox::new("doc_term_before_file");
    let doc = sandbox.compiler(&["doc", "--file", "missing.rslisp", "x y"]);
    assert_eq!(
        doc.assert_exit_code(2).stderr(),
        "error: `x y` is not one name\n"
    );
}

#[test]
fn doc_rejects_a_shadowing_doc_page_that_returns_no_string() {
    let sandbox = Sandbox::new("doc_shadowed").with_file("main.rslisp", SHADOWING_DOC_PAGE);
    let doc = sandbox.compiler(&["doc", "--file", "main.rslisp", "main"]);
    assert_eq!(
        doc.assert_exit_code(1).stderr(),
        "error: `doc-page` did not expand to a string\n"
    );
}

#[test]
fn doc_types_prints_the_inferred_type_above_the_page() {
    let sandbox = Sandbox::new("doc_types").with_file("prog.rslisp", TYPED_PROGRAM);
    assert_eq!(
        sandbox.typed_page(&["--file", "prog.rslisp", "twice"]),
        format!("{TWICE_TYPE}\n{TWICE_PAGE}")
    );
}

#[test]
fn doc_types_takes_the_flag_after_the_term() {
    let sandbox = Sandbox::new("doc_types_after_term").with_file("prog.rslisp", TYPED_PROGRAM);
    let doc = sandbox.compiler(&["doc", "twice", "--file", "prog.rslisp", "--types"]);
    assert_eq!(
        doc.assert_exit_code(0).stdout(),
        format!("{TWICE_TYPE}\n{TWICE_PAGE}")
    );
}

#[test]
fn doc_types_prints_the_generic_type_of_an_uncalled_function() {
    let sandbox = Sandbox::new("doc_types_generic").with_file("prog.rslisp", TYPED_PROGRAM);
    assert_eq!(
        sandbox.typed_page(&["--file", "prog.rslisp", "same"]),
        "(type same (where (a) (fn (a) a)))\n(defn same (x))\n\nNo documentation.\n"
    );
}

#[test]
fn doc_types_prints_no_type_for_a_macro() {
    let sandbox = Sandbox::new("doc_types_macro");
    assert_eq!(
        sandbox.typed_page(&["when"]),
        "(defrule when)\n\nNo documentation.\n"
    );
}

#[test]
fn doc_types_prints_no_type_for_a_kernel_form() {
    let sandbox = Sandbox::new("doc_types_kernel_form");
    assert_eq!(sandbox.typed_page(&["if"]), "if\n\nNo documentation.\n");
}

#[test]
fn doc_types_prints_the_type_of_a_prelude_function() {
    let sandbox = Sandbox::new("doc_types_prelude");
    assert_eq!(
        sandbox.typed_page(&["sum"]),
        "(type sum (where ((n number)) (fn ((iterator n)) n)))\n(defn sum (values))\n\nNo documentation.\n"
    );
}

#[test]
fn doc_types_prints_no_type_for_a_struct() {
    let sandbox = Sandbox::new("doc_types_struct").with_file("prog.rslisp", TYPED_PROGRAM);
    assert_eq!(
        sandbox.typed_page(&["--file", "prog.rslisp", "point"]),
        "(defstruct point)\n\nNo documentation.\n"
    );
}

#[test]
fn doc_types_prints_the_type_of_a_struct_constructor() {
    let sandbox = Sandbox::new("doc_types_constructor").with_file("prog.rslisp", TYPED_PROGRAM);
    assert_eq!(
        sandbox.typed_page(&["--file", "prog.rslisp", "->point"]),
        "(type ->point (fn (i64 i64) point))\n(defstruct point)\n\nNo documentation.\n"
    );
}

#[test]
fn doc_types_prints_the_type_of_a_function_with_keyword_parameters() {
    let sandbox = Sandbox::new("doc_types_keyword").with_file("prog.rslisp", TYPED_PROGRAM);
    assert_eq!(
        sandbox.typed_page(&["--file", "prog.rslisp", "area"]),
        "(type area (fn (i64 i64) i64))\n(defn area (:width i64 :height i64))\n\nNo documentation.\n"
    );
}

#[test]
fn doc_types_prints_the_type_of_a_plain_definition() {
    let sandbox = Sandbox::new("doc_types_plain").with_file("prog.rslisp", TYPED_PROGRAM);
    assert_eq!(
        sandbox.typed_page(&["--file", "prog.rslisp", "limit"]),
        "(type limit i64)\nlimit\n\nNo documentation.\n"
    );
}

#[test]
fn doc_types_prints_no_type_for_a_value_that_no_definition_binds() {
    let sandbox = Sandbox::new("doc_types_literal");
    assert_eq!(sandbox.typed_page(&["true"]), "true\n\nNo documentation.\n");
}

#[test]
fn doc_types_prints_no_type_above_a_documented_struct_beside_a_function() {
    let sandbox = Sandbox::new("doc_types_documented_struct")
        .with_file("prog.rslisp", DOCUMENTED_STRUCT_BESIDE_A_FUNCTION);
    assert_eq!(
        sandbox.typed_page(&["--file", "prog.rslisp", "point"]),
        "(defstruct point)\n\nA point on a grid.\n"
    );
}

#[test]
fn doc_types_prints_the_type_of_a_documented_function_beside_a_struct() {
    let sandbox = Sandbox::new("doc_types_documented_function")
        .with_file("prog.rslisp", DOCUMENTED_FUNCTION_BESIDE_A_STRUCT);
    assert_eq!(
        sandbox.typed_page(&["--file", "prog.rslisp", "point"]),
        format!("{POINT_TYPE}\n(defn point ())\n\nMakes the origin.\n")
    );
}

#[test]
fn doc_types_prints_the_type_of_an_undocumented_function_beside_a_struct() {
    let sandbox = Sandbox::new("doc_types_undocumented_function")
        .with_file("prog.rslisp", UNDOCUMENTED_FUNCTION_BESIDE_A_STRUCT);
    assert_eq!(
        sandbox.typed_page(&["--file", "prog.rslisp", "point"]),
        format!("{POINT_TYPE}\n(defn point ())\n\nNo documentation.\n")
    );
}

#[test]
fn doc_types_prints_the_type_of_an_undocumented_def_beside_a_struct() {
    let sandbox = Sandbox::new("doc_types_undocumented_def")
        .with_file("prog.rslisp", UNDOCUMENTED_DEF_BESIDE_A_STRUCT);
    assert_eq!(
        sandbox.typed_page(&["--file", "prog.rslisp", "point"]),
        "(type point i64)\npoint\n\nNo documentation.\n"
    );
}

#[test]
fn doc_types_prints_the_page_alone_when_another_definition_fails_to_type_check() {
    let sandbox =
        Sandbox::new("doc_types_other_fails").with_file("prog.rslisp", OTHER_DEFINITION_FAILS);
    let doc = sandbox.typed_lookup(&["--file", "prog.rslisp", "twice"]);
    assert!(doc.assert_exit_code(1).stderr().starts_with("error[L0401]"));
    assert_eq!(doc.stdout(), TWICE_PAGE);
}

#[test]
fn doc_types_prints_the_page_alone_when_its_own_definition_fails_to_type_check() {
    let sandbox =
        Sandbox::new("doc_types_own_fails").with_file("prog.rslisp", OWN_DEFINITION_FAILS);
    let doc = sandbox.typed_lookup(&["--file", "prog.rslisp", "twice"]);
    assert!(doc.assert_exit_code(1).stderr().starts_with("error[L0404]"));
    assert_eq!(doc.stdout(), TWICE_PAGE);
}

#[test]
fn doc_types_prints_the_type_of_an_imported_term() {
    let sandbox = Sandbox::new("doc_types_imported")
        .with_file("lib/numbers.rslisp", NUMBERS)
        .with_file("main.rslisp", NUMBERS_ENTRY);
    assert_eq!(
        sandbox.typed_page(&["--file", "main.rslisp", "twice"]),
        format!("{TWICE_TYPE}\n{TWICE_PAGE}")
    );
}

#[test]
fn doc_types_prints_the_type_of_the_definition_that_shadows_a_prelude_name() {
    let sandbox = Sandbox::new("doc_types_shadowing").with_file("main.rslisp", SHADOWING_SUM);
    assert_eq!(
        sandbox.typed_page(&["--file", "main.rslisp", "sum"]),
        "(type sum (fn (i64 i64) i64))\n(defn sum (left right))\n\nNo documentation.\n"
    );
}

#[test]
fn doc_types_leaves_a_page_in_the_program_untyped() {
    let sandbox = Sandbox::new("doc_types_in_program").with_file("prog.rslisp", PAGE_PROGRAM);
    let expanded = sandbox.compiler(&["expand", "prog.rslisp"]);
    assert!(
        expanded
            .assert_exit_code(0)
            .stdout()
            .contains(r#"(def page "(defn twice (x))\n\nDoubles a number.")"#)
    );
    assert_eq!(
        sandbox.typed_page(&["--file", "prog.rslisp", "page"]),
        "(type page (ref str))\npage\n\nNo documentation.\n"
    );
}

#[test]
fn emit_qualifies_a_struct_named_in_a_bound() {
    let sandbox = Sandbox::new("bound_import")
        .with_file("bound.rslisp", BOUND_ENTRY)
        .with_file("other/person.rslisp", BOUND_MODULE);
    let emitted = sandbox.compiler(&["emit", "bound.rslisp"]);
    let stdout = emitted.assert_exit_code(0).stdout();
    assert!(stdout.contains("std::ops::Add<crate::other::person::Person>"));
    assert!(stdout.contains("struct Person"));
}

#[test]
fn run_keeps_a_trait_method_vector_parameter_as_written() {
    let sandbox = Sandbox::new("trait_vector").with_file("mileage.rslisp", TRAIT_SHARED_VECTOR);
    let run = sandbox.compiler(&["run", "mileage.rslisp"]);
    assert_eq!(run.assert_exit_code(0).stdout(), "1210\n");
}

#[test]
fn run_panics_with_the_printed_form_of_an_unwrapped_error() {
    let sandbox = Sandbox::new("unwrapped_failure").with_file("failure.rslisp", UNWRAPPED_FAILURE);
    let run = sandbox.compiler(&["run", "failure.rslisp"]);
    assert!(
        run.assert_exit_code(101)
            .stderr()
            .contains("called unwrap-ok on (err (->failure :line 3))")
    );
}

#[test]
fn check_reports_errors_without_terminal_styling() {
    let sandbox = Sandbox::new("check_errors").with_file("unbound.rslisp", UNBOUND_NAME);
    let check = sandbox.compiler(&["check", "unbound.rslisp"]);
    insta::assert_snapshot!(check.assert_exit_code(1).stderr());
}

#[test]
fn check_caps_the_printed_diagnostics_and_counts_the_rest() {
    let sandbox =
        Sandbox::new("check_capped").with_unbound_names("flood.rslisp", UNBOUND_NAMES_PAST_THE_CAP);
    let check = sandbox.compiler(&["check", "flood.rslisp"]);
    insta::assert_snapshot!(check.assert_exit_code(1).stderr());
}

#[test]
fn expand_prints_kernel_forms() {
    let sandbox = Sandbox::new("expand").with_file("greeting.rslisp", GREETING);
    let expand = sandbox.compiler(&["expand", "greeting.rslisp"]);
    assert_eq!(
        expand.assert_exit_code(0).stdout(),
        "(def main (fn () ((rust \"println!(\\\"hello from the sandbox\\\")\" (where () (fn () unit))))))\n"
    );
}

#[test]
fn emit_prints_the_generated_rust() {
    let sandbox = Sandbox::new("emit").with_file("greeting.rslisp", GREETING);
    let emit = sandbox.compiler(&["emit", "greeting.rslisp"]);
    assert!(emit.assert_exit_code(0).stdout().contains("fn main() {"));
}

#[test]
fn build_writes_the_program_under_the_build_directory() {
    let sandbox = Sandbox::new("build_default").with_file("greeting.rslisp", GREETING);
    sandbox
        .compiler(&["build", "greeting.rslisp"])
        .assert_exit_code(0);
    let greeting = sandbox.built_program("target/rslisp/greeting/greeting");
    assert_eq!(
        greeting.assert_exit_code(0).stdout(),
        "hello from the sandbox\n"
    );
}

#[test]
fn build_keeps_entries_with_the_same_stem_apart() {
    let sandbox = Sandbox::new("build_same_stem")
        .with_file("a/program.rslisp", PRINTS_A)
        .with_file("b/program.rslisp", PRINTS_B);
    sandbox
        .compiler(&["build", "a/program.rslisp"])
        .assert_exit_code(0);
    sandbox
        .compiler(&["build", "b/program.rslisp"])
        .assert_exit_code(0);
    let first = sandbox.built_program("target/rslisp/a/program/program");
    let second = sandbox.built_program("target/rslisp/b/program/program");
    assert_eq!(first.assert_exit_code(0).stdout(), "A\n");
    assert_eq!(second.assert_exit_code(0).stdout(), "B\n");
}

#[test]
fn build_mirrors_an_entry_outside_the_working_directory() {
    let sandbox = Sandbox::new("build_outside")
        .with_file("shared/program.rslisp", PRINTS_A)
        .with_file("work/empty", "");
    sandbox
        .compiler_in("work", &["build", "../shared/program.rslisp"])
        .assert_exit_code(0);
    let program =
        sandbox.built_program(sandbox.outside_build_path("work", "shared/program/program"));
    assert_eq!(program.assert_exit_code(0).stdout(), "A\n");
}

#[test]
fn built_program_stops_quietly_when_its_reader_closes_stdout() {
    let sandbox = Sandbox::new("closed_stdout").with_file("count.rslisp", MILLION_LINES);
    sandbox
        .compiler(&["build", "count.rslisp"])
        .assert_exit_code(0);
    let count = sandbox.built_program_read_for_one_line("target/rslisp/count/count");
    assert_eq!(count.assert_quiet_stop().stdout(), "0\n");
}

#[test]
fn built_program_ends_its_input_lines_after_a_failed_read() {
    let sandbox = Sandbox::new("failed_read").with_file("lines.rslisp", COUNT_INPUT_LINES);
    sandbox
        .compiler(&["build", "lines.rslisp"])
        .assert_exit_code(0);
    let lines = sandbox.built_program_reading_its_directory("target/rslisp/lines/lines");
    assert_eq!(lines.assert_exit_code(0).stdout(), "1\n");
}

#[test]
fn build_writes_the_program_to_the_output_path() {
    let sandbox = Sandbox::new("build_output").with_file("greeting.rslisp", GREETING);
    sandbox
        .compiler(&["build", "greeting.rslisp", "-o", "bin/hello"])
        .assert_exit_code(0);
    let greeting = sandbox.built_program("bin/hello");
    assert_eq!(
        greeting.assert_exit_code(0).stdout(),
        "hello from the sandbox\n"
    );
}

#[test]
fn emit_without_rustfmt_prints_one_note_and_writes_unformatted_rust() {
    let sandbox = Sandbox::new("emit_without_rustfmt").with_file("greeting.rslisp", GREETING);
    let emit = sandbox.compiler_with_path("no-tools", &["emit", "greeting.rslisp"]);
    let emit = emit.assert_exit_code(0);
    assert!(emit.stdout().contains("fn main() {"));
    assert_eq!(emit.stderr().matches("note:").count(), 1);
    assert!(emit.stderr().contains("rustfmt"));
    assert!(emit.stderr().contains("unformatted"));
}

#[test]
fn emit_with_a_failing_rustfmt_prints_one_note_and_writes_unformatted_rust() {
    let sandbox = Sandbox::new("emit_failing_rustfmt")
        .with_file("greeting.rslisp", GREETING)
        .with_executable("tools/rustfmt", FAILING_RUSTFMT);
    let emit = sandbox.compiler_with_path("tools", &["emit", "greeting.rslisp"]);
    let emit = emit.assert_exit_code(0);
    assert!(emit.stdout().contains("fn main() {"));
    assert_eq!(emit.stderr().matches("note:").count(), 1);
    assert!(
        emit.stderr().contains("rustfmt failed"),
        "stderr: {}",
        emit.stderr()
    );
}

struct NestedIfProgram;

impl NestedIfProgram {
    const DEPTH: usize = 25;

    fn source() -> String {
        let opening = "(if true ".repeat(Self::DEPTH);
        let closing = " 0)".repeat(Self::DEPTH);
        format!("(defn main ()\n  (println {opening}1{closing}))\n")
    }
}

#[test]
fn emit_stops_rustfmt_past_its_time_limit_and_writes_unformatted_rust() {
    let time_limit = Duration::from_millis(500);
    let sandbox = Sandbox::new("emit_rustfmt_time_limit")
        .with_file("nested.rslisp", &NestedIfProgram::source());
    let started = Instant::now();
    let emit = sandbox.compiler_with_rustfmt_time_limit(time_limit, &["emit", "nested.rslisp"]);
    assert!(started.elapsed() < time_limit + Duration::from_secs(5));
    let emit = emit.assert_exit_code(0);
    assert_eq!(emit.stderr().matches("note:").count(), 1);
    assert!(
        emit.stderr()
            .contains("rustfmt did not finish within 500ms"),
        "stderr: {}",
        emit.stderr()
    );
    assert_eq!(
        emit.stdout().matches("if true").count(),
        NestedIfProgram::DEPTH
    );
}

#[test]
fn emit_rejects_a_malformed_rustfmt_time_limit_as_a_usage_error() {
    let sandbox =
        Sandbox::new("emit_malformed_rustfmt_time_limit").with_file("greeting.rslisp", GREETING);
    let emit = sandbox.compiler_with_rustfmt_time_limit_value("soon", &["emit", "greeting.rslisp"]);
    let emit = emit.assert_exit_code(2);
    assert!(
        emit.stderr()
            .contains(&EmittedFormatting::time_limit_variable()),
        "stderr: {}",
        emit.stderr()
    );
}

#[test]
fn build_reports_rustc_errors_as_program_errors() {
    let sandbox = Sandbox::new("build_rustc_error").with_file("sign.rslisp", RUSTC_SYNTAX_ERROR);
    let build = sandbox.compiler(&["build", "sign.rslisp"]);
    assert!(
        build
            .assert_exit_code(1)
            .stderr()
            .contains("note: reported by rustc")
    );
}

#[test]
fn run_prints_the_program_output() {
    let sandbox = Sandbox::new("run").with_file("greeting.rslisp", GREETING);
    let run = sandbox.compiler(&["run", "--release", "greeting.rslisp"]);
    assert_eq!(run.assert_exit_code(0).stdout(), "hello from the sandbox\n");
}

#[test]
fn run_passes_the_arguments_after_the_separator() {
    let sandbox = Sandbox::new("run_arguments").with_file("echo.rslisp", ECHO);
    let run = sandbox.compiler(&["run", "echo.rslisp", "--", "fake-argument"]);
    assert_eq!(run.assert_exit_code(0).stdout(), "fake-argument\n");
}

#[test]
fn run_exits_with_the_program_exit_code() {
    let sandbox = Sandbox::new("run_exit_code").with_file("exit.rslisp", EXIT_THREE);
    sandbox
        .compiler(&["run", "exit.rslisp"])
        .assert_exit_code(3);
}

#[test]
fn run_ends_quietly_when_its_reader_closes() {
    let sandbox = Sandbox::new("run_closed_pipe").with_file("lines.rslisp", THOUSAND_LINES);
    sandbox
        .compiler_writing_to_a_closed_pipe(&["run", "lines.rslisp"])
        .assert_silent_success();
}

#[test]
fn run_fails_when_the_program_dies_of_another_signal() {
    let sandbox = Sandbox::new("run_abort").with_file("abort.rslisp", ABORT);
    let run = sandbox.compiler(&["run", "abort.rslisp"]);
    assert_eq!(run.assert_exit_code(1).stderr(), "");
}

#[test]
fn run_prints_a_main_error_with_display_and_exits_one() {
    let sandbox = Sandbox::new("run_main_error").with_file("main_error.rslisp", MAIN_ERROR);
    let run = sandbox.compiler(&["run", "main_error.rslisp"]);
    run.assert_exit_code(1);
    let stderr = run.stderr();
    assert!(
        stderr.contains("No such file or directory"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("Os {"), "stderr: {stderr}");
}

#[test]
fn every_command_reports_program_errors() {
    let sandbox = Sandbox::new("program_errors").with_file("unbound.rslisp", UNBOUND_NAME);
    for command in ["expand", "emit", "build", "run"] {
        sandbox
            .compiler(&[command, "unbound.rslisp"])
            .assert_exit_code(1);
    }
}

#[test]
fn build_without_rustc_is_a_usage_error() {
    let sandbox = Sandbox::new("build_without_rustc")
        .with_file("greeting.rslisp", GREETING)
        .with_executable("tools/rustfmt", PASSTHROUGH_RUSTFMT);
    let build = sandbox.compiler_with_path("tools", &["build", "greeting.rslisp"]);
    assert!(
        build
            .assert_exit_code(2)
            .stderr()
            .starts_with("error: cannot run rustc")
    );
}

#[test]
fn build_cannot_write_under_a_file() {
    let sandbox = Sandbox::new("build_under_file")
        .with_file("greeting.rslisp", GREETING)
        .with_file("blocker", "");
    let build = sandbox.compiler(&["build", "greeting.rslisp", "-o", "blocker/program"]);
    assert!(
        build
            .assert_exit_code(2)
            .stderr()
            .starts_with("error: cannot create `blocker`")
    );
}

#[test]
fn build_reports_a_rustc_failure_without_diagnostics() {
    let sandbox = Sandbox::new("build_rustc_crash")
        .with_file("greeting.rslisp", GREETING)
        .with_executable("tools/rustc", CRASHING_RUSTC)
        .with_executable("tools/rustfmt", PASSTHROUGH_RUSTFMT);
    let build = sandbox.compiler_with_path("tools", &["build", "greeting.rslisp"]);
    insta::assert_snapshot!(build.assert_exit_code(1).stderr());
}

#[test]
fn run_reports_a_program_that_cannot_start() {
    let sandbox = Sandbox::new("run_missing_program")
        .with_file("greeting.rslisp", GREETING)
        .with_executable("tools/rustc", SILENT_RUSTC)
        .with_executable("tools/rustfmt", PASSTHROUGH_RUSTFMT);
    let run = sandbox.compiler_with_path("tools", &["run", "greeting.rslisp"]);
    assert!(
        run.assert_exit_code(2)
            .stderr()
            .starts_with("error: cannot run `target/rslisp/greeting/greeting`")
    );
}

#[test]
fn eval_prints_a_value_through_prn() {
    let sandbox = Sandbox::new("eval_value").with_file("session.rslisp", SESSION);
    let eval = sandbox.compiler(&["eval", "session.rslisp", "(->point (square 2) 3)"]);
    assert_eq!(
        eval.assert_silent_success().stdout(),
        "(->point :x 4 :y 3)\n"
    );
}

#[test]
fn eval_prints_nothing_for_a_unit_result() {
    let sandbox = Sandbox::new("eval_unit").with_file("session.rslisp", SESSION);
    let eval = sandbox.compiler(&["eval", "session.rslisp", "(println \"side \" (square 3))"]);
    assert_eq!(eval.assert_silent_success().stdout(), "side 9\n");
}

#[test]
fn eval_runs_every_form_once_in_order_as_one_body() {
    let sandbox = Sandbox::new("eval_body").with_file("session.rslisp", SESSION);
    let eval = sandbox.compiler(&[
        "eval",
        "session.rslisp",
        "(let x 5) (println \"x\") (* x 2) (main)",
    ]);
    assert_eq!(
        eval.assert_silent_success().stdout(),
        "x\n10\nwritten main\n"
    );
}

#[test]
fn eval_notes_a_value_whose_type_does_not_implement_show() {
    let sandbox = Sandbox::new("eval_no_show").with_file("session.rslisp", SESSION);
    let eval = sandbox.compiler(&["eval", "session.rslisp", "(square 2) (fn ((x i64)) x)"]);
    insta::assert_snapshot!(eval.assert_exit_code(0).stderr());
    assert_eq!(eval.stdout(), "4\n");
}

#[test]
fn eval_reports_an_open_type_on_the_form() {
    let sandbox = Sandbox::new("eval_open_type").with_file("session.rslisp", SESSION);
    let eval = sandbox.compiler(&["eval", "session.rslisp", "(println \"never\") none"]);
    insta::assert_snapshot!(eval.assert_exit_code(1).stderr());
    assert_eq!(eval.stdout(), "");
}

#[test]
fn eval_reports_a_diagnostic_of_the_module_and_runs_nothing() {
    let sandbox = Sandbox::new("eval_module_error").with_file("geometry.rslisp", AREA_OF_A_STRING);
    let eval = sandbox.compiler(&["eval", "geometry.rslisp", "(println \"never\")"]);
    insta::assert_snapshot!(eval.assert_exit_code(1).stderr());
    assert_eq!(eval.stdout(), "");
}

#[test]
fn eval_reports_a_diagnostic_of_a_form_at_its_own_line() {
    let sandbox =
        Sandbox::new("eval_form_error").with_file("geometry.rslisp", TOP_LEVEL_AREA_OF_A_STRING);
    let eval = sandbox.compiler(&[
        "eval",
        "--line",
        "2",
        "--column",
        "1",
        "geometry.rslisp",
        "(area 2 \"3\")",
    ]);
    insta::assert_snapshot!(eval.assert_exit_code(1).stderr());
}

#[test]
fn eval_reports_a_diagnostic_inside_a_definition_once() {
    let sandbox = Sandbox::new("eval_form_in_definition_error")
        .with_file("geometry.rslisp", AREA_OF_A_STRING);
    let eval = sandbox.compiler(&[
        "eval",
        "--line",
        "2",
        "--column",
        "17",
        "geometry.rslisp",
        "(area 2 \"3\")",
    ]);
    insta::assert_snapshot!(eval.assert_exit_code(1).stderr());
}

#[test]
fn eval_runs_a_top_level_form_that_the_module_check_leaves_out() {
    let sandbox = Sandbox::new("eval_top_level").with_file("geometry.rslisp", EVALUATED_GEOMETRY);
    let eval = sandbox.compiler(&[
        "eval",
        "--line",
        "2",
        "--column",
        "1",
        "geometry.rslisp",
        "(area 2 3)",
    ]);
    assert_eq!(eval.assert_silent_success().stdout(), "6\n");
}

#[test]
fn eval_notes_a_definition_form_and_runs_nothing() {
    let sandbox =
        Sandbox::new("eval_definition").with_file("geometry.rslisp", GEOMETRY_DEFINITIONS);
    let eval = sandbox.compiler(&[
        "eval",
        "--line",
        "1",
        "--column",
        "1",
        "geometry.rslisp",
        "(defn area (w h) (* w h))",
    ]);
    insta::assert_snapshot!(eval.assert_exit_code(0).stderr());
    assert_eq!(eval.stdout(), "");
}

#[test]
fn eval_notes_a_definition_inside_a_top_level_do() {
    let sandbox =
        Sandbox::new("eval_definition_in_do").with_file("geometry.rslisp", GEOMETRY_DEFINITIONS);
    let eval = sandbox.compiler(&[
        "eval",
        "--line",
        "4",
        "--column",
        "5",
        "geometry.rslisp",
        "(defn twice (n) (* 2 n))",
    ]);
    insta::assert_snapshot!(eval.assert_exit_code(0).stderr());
}

#[test]
fn eval_runs_a_form_inside_a_definition() {
    let sandbox = Sandbox::new("eval_inside").with_file("geometry.rslisp", GEOMETRY_DEFINITIONS);
    let eval = sandbox.compiler(&[
        "eval",
        "--line",
        "3",
        "--column",
        "12",
        "geometry.rslisp",
        "(area 4 5)",
    ]);
    assert_eq!(eval.assert_silent_success().stdout(), "20\n");
}

#[test]
fn eval_rejects_forms_that_are_not_the_text_at_their_position() {
    let sandbox = Sandbox::new("eval_misplaced").with_file("geometry.rslisp", EVALUATED_GEOMETRY);
    let eval = sandbox.compiler(&[
        "eval",
        "--line",
        "2",
        "--column",
        "2",
        "geometry.rslisp",
        "(area 2 3)",
    ]);
    assert_eq!(
        eval.assert_exit_code(2).stderr(),
        "error: the forms are not the text of `geometry.rslisp` at 2:2\n"
    );
}

#[test]
fn eval_rejects_a_line_without_a_column() {
    let sandbox = Sandbox::new("eval_line_only").with_file("session.rslisp", SESSION);
    let eval = sandbox.compiler(&["eval", "--line", "2", "session.rslisp", "1"]);
    assert!(
        eval.assert_exit_code(2)
            .stderr()
            .contains("--column <COLUMN>")
    );
}

#[test]
fn eval_reads_the_module_from_standard_input() {
    let sandbox = Sandbox::new("eval_stdin");
    let eval = sandbox.compiler_reading(
        SESSION,
        &["eval", "--stdin", "unsaved.rslisp", "(square 7)"],
    );
    assert_eq!(eval.assert_silent_success().stdout(), "49\n");
}

#[test]
fn eval_resolves_use_from_the_root() {
    let sandbox = Sandbox::new("eval_root")
        .with_file("shapes.rslisp", SHAPES)
        .with_file("src/user.rslisp", SHAPES_USER);
    let eval = sandbox.compiler(&["eval", "--root", ".", "src/user.rslisp", "(circle 2)"]);
    assert_eq!(eval.assert_silent_success().stdout(), "12\n");
}

#[test]
fn eval_exits_with_the_program_exit_code() {
    let sandbox = Sandbox::new("eval_exit").with_file("exits.rslisp", EXITS);
    sandbox
        .compiler(&["eval", "exits.rslisp", "(exit 3)"])
        .assert_exit_code(3);
}

#[test]
fn eval_exits_with_the_code_of_a_panic() {
    let sandbox = Sandbox::new("eval_panic").with_file("session.rslisp", SESSION);
    let eval = sandbox.compiler(&["eval", "session.rslisp", "(at (vec 1) 5)"]);
    assert!(
        eval.assert_exit_code(101)
            .stderr()
            .contains("index out of bounds")
    );
}

#[test]
fn eval_exits_with_128_plus_the_signal_that_stops_the_program() {
    let sandbox = Sandbox::new("eval_signal").with_file("exits.rslisp", EXITS);
    sandbox
        .compiler(&["eval", "exits.rslisp", "(abort)"])
        .assert_exit_code(134);
}

#[test]
fn eval_succeeds_when_its_output_closes() {
    let sandbox = Sandbox::new("eval_closed_output").with_file("session.rslisp", SESSION);
    let forms = "(for-each (range 0 1000000) (fn (i) (println i)))";
    sandbox
        .compiler_writing_to_a_closed_pipe(&["eval", "session.rslisp", forms])
        .assert_exit_code(0);
}

#[test]
fn eval_leaves_no_build_directory_beside_the_module() {
    let sandbox = Sandbox::new("eval_no_target").with_file("session.rslisp", SESSION);
    sandbox
        .compiler(&["eval", "session.rslisp", "(square 2)"])
        .assert_exit_code(0);
    assert!(!sandbox.directory.join("target").exists());
}

#[test]
fn eval_prints_a_warning_of_the_module_once_beside_a_warning_of_a_form() {
    let sandbox = Sandbox::new("eval_warnings").with_file("pick.rslisp", UNREACHABLE_ARM);
    let eval = sandbox.compiler(&["eval", "pick.rslisp", "(match 3 (_ 1) (_ 2))"]);
    let stderr = eval.assert_exit_code(0).stderr();
    assert_eq!(stderr.matches("--> pick.rslisp").count(), 1, "{stderr}");
    assert_eq!(stderr.matches("--> <eval>").count(), 1, "{stderr}");
    assert_eq!(eval.stdout(), "1\n");
}

#[test]
fn eval_prints_a_borrowed_value() {
    let sandbox = Sandbox::new("eval_borrowed").with_file("session.rslisp", SESSION);
    let eval = sandbox.compiler(&["eval", "session.rslisp", "(as-str (str \"ab\"))"]);
    assert_eq!(eval.assert_silent_success().stdout(), "\"ab\"\n");
}

#[test]
fn eval_notes_a_top_level_do_of_definitions() {
    let sandbox =
        Sandbox::new("eval_do_of_definitions").with_file("geometry.rslisp", GEOMETRY_DEFINITIONS);
    let eval = sandbox.compiler(&[
        "eval",
        "--line",
        "4",
        "--column",
        "1",
        "geometry.rslisp",
        "(do (defn twice (n) (* 2 n)))",
    ]);
    insta::assert_snapshot!(eval.assert_exit_code(0).stderr());
}

#[test]
fn eval_runs_a_top_level_do_of_expressions() {
    let sandbox = Sandbox::new("eval_do_of_expressions")
        .with_file("square.rslisp", TOP_LEVEL_DO_OF_EXPRESSIONS);
    let eval = sandbox.compiler(&[
        "eval",
        "--line",
        "2",
        "--column",
        "1",
        "square.rslisp",
        "(do (prn (square 3)))",
    ]);
    assert_eq!(eval.assert_silent_success().stdout(), "9\n");
}

#[test]
fn eval_prints_a_warning_of_the_module_once_beside_a_failing_form() {
    let sandbox = Sandbox::new("eval_warning_beside_error")
        .with_file("pick.rslisp", UNREACHABLE_ARM_IN_A_BODY);
    let eval = sandbox.compiler(&["eval", "pick.rslisp", "(square \"a\")"]);
    let stderr = eval.assert_exit_code(1).stderr();
    assert_eq!(
        stderr.matches("unreachable `match` arm").count(),
        1,
        "{stderr}"
    );
}

#[test]
fn eval_prints_a_warning_of_a_form_inside_a_definition_once() {
    let sandbox =
        Sandbox::new("eval_warning_inside").with_file("pick.rslisp", UNREACHABLE_ARM_IN_A_BODY);
    let eval = sandbox.compiler(&[
        "eval",
        "--line",
        "3",
        "--column",
        "3",
        "pick.rslisp",
        "(match 3 (_ 1) (_ 2))",
    ]);
    let stderr = eval.assert_exit_code(0).stderr();
    assert_eq!(
        stderr.matches("unreachable `match` arm").count(),
        1,
        "{stderr}"
    );
    assert_eq!(eval.stdout(), "1\n");
}

#[test]
fn eval_prints_a_value_behind_a_mutable_borrow() {
    let sandbox = Sandbox::new("eval_mutable").with_file("session.rslisp", SESSION);
    let eval = sandbox.compiler(&["eval", "session.rslisp", "(let v (vec 1)) (mut v)"]);
    assert_eq!(eval.assert_silent_success().stdout(), "(vec 1)\n");
}

#[test]
fn eval_reports_an_expression_beside_a_definition_in_a_top_level_do() {
    let sandbox = Sandbox::new("eval_mixed_do").with_file("mixed.rslisp", MIXED_TOP_LEVEL_DO);
    let eval = sandbox.compiler(&[
        "eval",
        "--line",
        "2",
        "--column",
        "1",
        "mixed.rslisp",
        "(do (defn cube (n) (* n (square n))) (prn 7))",
    ]);
    insta::assert_snapshot!(eval.assert_exit_code(1).stderr());
    assert_eq!(eval.stdout(), "");
}

#[test]
fn eval_prints_a_warning_of_a_form_at_the_range_of_a_module_warning() {
    let sandbox = Sandbox::new("eval_warning_same_range").with_file("pick.rslisp", UNREACHABLE_ARM);
    let indented = format!("{}(match 3 (_ 1) (_ 2))", " ".repeat(15));
    let eval = sandbox.compiler(&["eval", "pick.rslisp", &indented]);
    let stderr = eval.assert_exit_code(0).stderr();
    assert_eq!(stderr.matches("--> pick.rslisp").count(), 1, "{stderr}");
    assert_eq!(stderr.matches("--> <eval>").count(), 1, "{stderr}");
}

#[test]
fn eval_json_streams_output_and_values_in_order() {
    let sandbox = Sandbox::new("eval_json_streams").with_file("session.rslisp", SESSION);
    let forms = "(do (print \"x\") 6) (println \"hi\") (square 3)";
    let eval = sandbox.compiler(&["eval", "--json", "session.rslisp", forms]);
    assert_eq!(
        eval.assert_silent_success().json_replies(),
        [
            json!({"out": "x"}),
            json!({"value": "6", "form": 1}),
            json!({"out": "hi\n"}),
            json!({"value": "9", "form": 3}),
            json!({"status": "done", "exit": 0}),
        ]
    );
}

#[test]
fn eval_json_echoes_the_id_on_every_object() {
    let sandbox = Sandbox::new("eval_json_id").with_file("session.rslisp", SESSION);
    let eval = sandbox.compiler(&[
        "eval",
        "--json",
        "--id",
        "7",
        "session.rslisp",
        "(println \"a\") (square 2)",
    ]);
    let replies = eval.assert_silent_success().json_replies();
    assert_eq!(replies.len(), 3);
    assert!(
        replies.iter().all(|reply| reply["id"] == "7"),
        "{replies:?}"
    );
}

#[test]
fn eval_json_numbers_a_value_by_its_form_among_definitions() {
    let sandbox =
        Sandbox::new("eval_json_numbers").with_file("geometry.rslisp", EVALUATED_GEOMETRY);
    let eval = sandbox.compiler(&[
        "eval",
        "--json",
        "--line",
        "1",
        "--column",
        "1",
        "geometry.rslisp",
        "(defn area (w h) (* w h))\n(area 2 3)",
    ]);
    let replies = eval.assert_exit_code(0).json_replies();
    assert_eq!(replies[1], json!({"value": "6", "form": 2}));
}

#[test]
fn eval_json_prints_a_value_that_borrows_a_temporary() {
    let sandbox = Sandbox::new("eval_json_temporary").with_file("session.rslisp", SESSION);
    let forms = "(trim (as-str (str \" a\" \"b \")))";
    let eval = sandbox.compiler(&["eval", "--json", "session.rslisp", forms]);
    assert_eq!(
        eval.assert_silent_success().json_replies()[0],
        json!({"value": "\"ab\"", "form": 1})
    );
}

#[test]
fn eval_json_streams_a_doc_page_as_out() {
    let sandbox = Sandbox::new("eval_json_doc").with_file("page.rslisp", PAGE_PROGRAM);
    let eval = sandbox.compiler(&["eval", "--json", "page.rslisp", "(doc twice)"]);
    assert_eq!(
        eval.assert_silent_success().json_replies(),
        [
            json!({"out": TWICE_PAGE}),
            json!({"status": "done", "exit": 0})
        ]
    );
}

#[test]
fn eval_json_streams_standard_error_as_err() {
    let sandbox = Sandbox::new("eval_json_err").with_file("session.rslisp", SESSION);
    let eval = sandbox.compiler(&["eval", "--json", "session.rslisp", "(eprintln \"oops\")"]);
    assert_eq!(
        eval.assert_silent_success().json_replies(),
        [
            json!({"err": "oops\n"}),
            json!({"status": "done", "exit": 0})
        ]
    );
}

#[test]
fn eval_json_reports_a_note_as_a_diagnostic_without_a_code() {
    let sandbox = Sandbox::new("eval_json_note").with_file("session.rslisp", SESSION);
    let eval = sandbox.compiler(&["eval", "--json", "session.rslisp", "(fn ((x i64)) x)"]);
    let replies = eval.assert_silent_success().json_replies();
    let note = &replies[0]["diagnostics"][0];
    assert_eq!(note["severity"], "note");
    assert!(note.get("code").is_none(), "{note}");
    assert_eq!(
        (
            &note["file"],
            &note["line"],
            &note["column"],
            &note["end_column"]
        ),
        (&json!("<eval>"), &json!(1), &json!(1), &json!(17))
    );
    assert!(
        note["rendered"]
            .as_str()
            .is_some_and(|rendered| rendered.starts_with("note: the value has type"))
    );
    assert_eq!(replies[1], json!({"status": "done", "exit": 0}));
}

#[test]
fn eval_json_places_an_error_in_the_module_text_in_bytes() {
    let sandbox = Sandbox::new("eval_json_error").with_file(
        "geometry.rslisp",
        "(defn area (w h) (* w h))\n(do \"☃\" (area 2 \"3\"))\n",
    );
    let eval = sandbox.compiler(&[
        "eval",
        "--json",
        "--line",
        "2",
        "--column",
        "1",
        "geometry.rslisp",
        "(do \"☃\" (area 2 \"3\"))",
    ]);
    let replies = eval.assert_exit_code(1).json_replies();
    let error = &replies[0]["diagnostics"][0];
    assert_eq!(
        (&error["severity"], &error["code"], &error["file"]),
        (&json!("error"), &json!("L0401"), &json!("geometry.rslisp"))
    );
    assert_eq!(
        (
            &error["line"],
            &error["column"],
            &error["end_line"],
            &error["end_column"]
        ),
        (&json!(2), &json!(19), &json!(2), &json!(22))
    );
    assert!(eval.stdout().starts_with(
        r#"{"diagnostics":[{"severity":"error","code":"L0401","message":"mismatched types","file":"geometry.rslisp","line":2,"column":19,"end_line":2,"end_column":22,"rendered":"#
    ));
    assert_eq!(replies[1], json!({"status": "done", "exit": 1}));
}

#[test]
fn eval_json_reports_a_diagnostic_of_the_module_and_runs_nothing() {
    let sandbox =
        Sandbox::new("eval_json_module_error").with_file("geometry.rslisp", AREA_OF_A_STRING);
    let eval = sandbox.compiler(&["eval", "--json", "geometry.rslisp", "(println \"never\")"]);
    let replies = eval.assert_exit_code(1).json_replies();
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert_eq!(replies[0]["diagnostics"][0]["code"], "L0401");
    assert_eq!(replies[1], json!({"status": "done", "exit": 1}));
    assert_eq!(eval.stderr(), "");
}

#[test]
fn eval_json_ends_with_the_exit_code_of_the_program() {
    let sandbox = Sandbox::new("eval_json_exit").with_file("exits.rslisp", EXITS);
    let eval = sandbox.compiler(&["eval", "--json", "exits.rslisp", "(exit 3)"]);
    assert_eq!(
        eval.assert_exit_code(3).json_replies(),
        [json!({"status": "done", "exit": 3})]
    );
}

#[test]
fn eval_json_ends_with_128_plus_the_signal_that_stops_the_program() {
    let sandbox = Sandbox::new("eval_json_signal").with_file("exits.rslisp", EXITS);
    let eval = sandbox.compiler(&["eval", "--json", "exits.rslisp", "(abort)"]);
    assert_eq!(
        eval.assert_exit_code(134).json_replies(),
        [json!({"status": "done", "exit": 134})]
    );
}

#[test]
fn eval_json_succeeds_when_its_output_closes() {
    let sandbox = Sandbox::new("eval_json_closed_output").with_file("session.rslisp", SESSION);
    let forms = "(for-each (range 0 1000000) (fn (i) (println i)))";
    sandbox
        .compiler_writing_to_a_closed_pipe(&["eval", "--json", "session.rslisp", forms])
        .assert_exit_code(0);
}

#[test]
fn eval_json_writes_no_json_for_misplaced_forms() {
    let sandbox =
        Sandbox::new("eval_json_misplaced").with_file("geometry.rslisp", EVALUATED_GEOMETRY);
    let eval = sandbox.compiler(&[
        "eval",
        "--json",
        "--line",
        "9",
        "--column",
        "1",
        "geometry.rslisp",
        "(area 2 3)",
    ]);
    assert_eq!(eval.assert_exit_code(2).stdout(), "");
    assert!(
        eval.stderr()
            .starts_with("error: the forms are not the text")
    );
}

#[test]
fn eval_json_writes_no_json_for_a_missing_module() {
    let sandbox = Sandbox::new("eval_json_missing");
    let eval = sandbox.compiler(&["eval", "--json", "missing.rslisp", "1"]);
    assert_eq!(eval.assert_exit_code(2).stdout(), "");
    assert!(
        eval.stderr()
            .starts_with("error: cannot read `missing.rslisp`")
    );
}

#[test]
fn eval_rejects_an_id_without_json() {
    let sandbox = Sandbox::new("eval_id_without_json").with_file("session.rslisp", SESSION);
    let eval = sandbox.compiler(&["eval", "--id", "7", "session.rslisp", "1"]);
    assert_eq!(eval.assert_exit_code(2).stdout(), "");
}

#[test]
fn eval_json_prints_a_value_beside_a_module_println() {
    let sandbox = Sandbox::new("eval_json_shadowed").with_file("square.rslisp", SHADOWED_PRINTLN);
    let eval = sandbox.compiler(&["eval", "--json", "square.rslisp", "(square 2)"]);
    assert_eq!(
        eval.assert_silent_success().json_replies()[0],
        json!({"value": "4", "form": 1})
    );
}

#[test]
fn eval_json_reports_a_warning_with_its_severity_and_code() {
    let sandbox = Sandbox::new("eval_json_warning").with_file("pick.rslisp", UNREACHABLE_ARM);
    let eval = sandbox.compiler(&["eval", "--json", "pick.rslisp", "(do)"]);
    let replies = eval.assert_silent_success().json_replies();
    let warning = &replies[0]["diagnostics"][0];
    assert_eq!(warning["severity"], "warning");
    assert!(warning["code"].is_string(), "{warning}");
}

#[test]
fn eval_json_stops_a_program_whose_output_closes() {
    let sandbox = Sandbox::new("eval_json_closed_count").with_file("count.rslisp", COUNT_THEN_FAIL);
    sandbox
        .compiler_writing_to_a_closed_pipe(&["eval", "--json", "count.rslisp", "(count-up 0)"])
        .assert_exit_code(0);
}

#[test]
fn eval_json_ends_with_status_2_for_a_program_that_cannot_start() {
    let sandbox = Sandbox::new("eval_json_missing_program")
        .with_file("session.rslisp", SESSION)
        .with_executable("tools/rustc", SILENT_RUSTC)
        .with_executable("tools/rustfmt", PASSTHROUGH_RUSTFMT);
    let eval = sandbox.compiler_with_path("tools", &["eval", "--json", "session.rslisp", "1"]);
    assert_eq!(
        eval.assert_exit_code(2).json_replies(),
        [json!({"status": "done", "exit": 2})]
    );
    assert!(eval.stderr().starts_with("error: cannot run"));
}

#[test]
fn eval_json_ends_with_status_2_without_rustc() {
    let sandbox = Sandbox::new("eval_json_missing_rustc")
        .with_file("pick.rslisp", UNREACHABLE_ARM)
        .with_executable("tools/rustfmt", PASSTHROUGH_RUSTFMT);
    let eval = sandbox.compiler_with_path("tools", &["eval", "--json", "pick.rslisp", "(pick 1)"]);
    let replies = eval.assert_exit_code(2).json_replies();
    assert_eq!(replies[0]["diagnostics"][0]["severity"], "warning");
    assert_eq!(replies[1], json!({"status": "done", "exit": 2}));
}

#[test]
fn eval_json_ends_with_status_2_without_a_build_directory() {
    let sandbox = Sandbox::new("eval_json_no_temporary").with_file("session.rslisp", SESSION);
    let eval = sandbox
        .compiler_with_temporary_directory("missing", &["eval", "--json", "session.rslisp", "1"]);
    assert_eq!(
        eval.assert_exit_code(2).json_replies(),
        [json!({"status": "done", "exit": 2})]
    );
    assert!(
        eval.stderr()
            .starts_with("error: cannot create a build directory")
    );
}

#[test]
fn eval_json_prints_values_of_two_types_in_one_call() {
    let sandbox = Sandbox::new("eval_json_two_types").with_file("session.rslisp", SESSION);
    let eval = sandbox.compiler(&[
        "eval",
        "--json",
        "session.rslisp",
        "(square 3) (->point :x 4 :y 3)",
    ]);
    assert_eq!(
        eval.assert_silent_success().json_replies(),
        [
            json!({"value": "9", "form": 1}),
            json!({"value": "(->point :x 4 :y 3)", "form": 2}),
            json!({"status": "done", "exit": 0}),
        ]
    );
}

#[test]
fn eval_json_reports_a_reader_error_in_the_forms() {
    let sandbox = Sandbox::new("eval_json_reader_error").with_file("session.rslisp", SESSION);
    let eval = sandbox.compiler(&["eval", "--json", "session.rslisp", "(square 2"]);
    let replies = eval.assert_exit_code(1).json_replies();
    assert_eq!(replies[0]["diagnostics"][0]["code"], "L0101");
    assert_eq!(replies[1], json!({"status": "done", "exit": 1}));
    assert_eq!(eval.stderr(), "");
}

#[test]
fn color_diagnostics_on_a_terminal() {
    let sandbox = Sandbox::new("color_terminal").with_file("unbound.rslisp", UNBOUND_NAME);
    let check = sandbox.compiler_on_a_terminal(&[("TERM", "xterm")], &["check", "unbound.rslisp"]);
    assert!(check.stdout_is_colored(), "{}", check.stdout());
}

#[test]
fn color_stays_off_diagnostics_on_a_dumb_terminal() {
    let sandbox = Sandbox::new("color_dumb").with_file("unbound.rslisp", UNBOUND_NAME);
    let check = sandbox.compiler_on_a_terminal(&[("TERM", "dumb")], &["check", "unbound.rslisp"]);
    assert!(
        check.stdout().contains("error[L0201]"),
        "{}",
        check.stdout()
    );
    assert!(!check.stdout_is_colored(), "{}", check.stdout());
}

#[test]
fn color_stays_off_diagnostics_on_a_terminal_when_clicolor_is_zero() {
    let sandbox = Sandbox::new("color_clicolor").with_file("unbound.rslisp", UNBOUND_NAME);
    let environment = [("TERM", "xterm"), ("CLICOLOR", "0")];
    let check = sandbox.compiler_on_a_terminal(&environment, &["check", "unbound.rslisp"]);
    assert!(
        check.stdout().contains("error[L0201]"),
        "{}",
        check.stdout()
    );
    assert!(!check.stdout_is_colored(), "{}", check.stdout());
}

#[test]
fn color_stays_off_diagnostics_on_a_terminal_when_no_color_is_set() {
    let sandbox = Sandbox::new("color_no_color").with_file("unbound.rslisp", UNBOUND_NAME);
    let environment = [("TERM", "xterm"), ("NO_COLOR", "1")];
    let check = sandbox.compiler_on_a_terminal(&environment, &["check", "unbound.rslisp"]);
    assert!(
        check.stdout().contains("error[L0201]"),
        "{}",
        check.stdout()
    );
    assert!(!check.stdout_is_colored(), "{}", check.stdout());
}

#[test]
fn color_stays_off_diagnostics_written_to_a_file_from_a_terminal() {
    let sandbox = Sandbox::new("color_file").with_file("unbound.rslisp", UNBOUND_NAME);
    let arguments = ["check", "unbound.rslisp", "2>", "errors.log"];
    sandbox.compiler_on_a_terminal(&[("TERM", "xterm")], &arguments);
    let errors = sandbox.written_file("errors.log");
    assert!(errors.contains("error[L0201]"), "{errors}");
    assert!(!errors.contains(Outcome::ESCAPE), "{errors}");
}

#[test]
fn color_stays_off_diagnostics_in_a_pipe() {
    let sandbox = Sandbox::new("color_pipe").with_file("unbound.rslisp", UNBOUND_NAME);
    let check = sandbox.compiler_with_colors(&[("TERM", "xterm")], &["check", "unbound.rslisp"]);
    assert!(
        !check.assert_exit_code(1).stderr_is_colored(),
        "{}",
        check.stderr()
    );
}

#[test]
fn color_diagnostics_in_a_pipe_when_clicolor_force_is_set() {
    let sandbox = Sandbox::new("color_force").with_file("unbound.rslisp", UNBOUND_NAME);
    let check =
        sandbox.compiler_with_colors(&[("CLICOLOR_FORCE", "1")], &["check", "unbound.rslisp"]);
    assert!(
        check.assert_exit_code(1).stderr_is_colored(),
        "{}",
        check.stderr()
    );
}

#[test]
fn color_no_color_wins_over_clicolor_force() {
    let sandbox = Sandbox::new("color_no_color_force").with_file("unbound.rslisp", UNBOUND_NAME);
    let environment = [("CLICOLOR_FORCE", "1"), ("NO_COLOR", "1")];
    let check = sandbox.compiler_with_colors(&environment, &["check", "unbound.rslisp"]);
    assert!(
        !check.assert_exit_code(1).stderr_is_colored(),
        "{}",
        check.stderr()
    );
}

#[test]
fn color_follows_the_decision_in_a_doc_term_that_does_not_read() {
    let sandbox = Sandbox::new("color_doc_term");
    let doc = sandbox.compiler_with_colors(&[("CLICOLOR_FORCE", "1")], &["doc", "("]);
    assert!(
        doc.assert_exit_code(2).stderr_is_colored(),
        "{}",
        doc.stderr()
    );
}

#[test]
fn color_follows_the_decision_in_eval_forms_that_do_not_read() {
    let sandbox = Sandbox::new("color_eval_forms").with_file("session.rslisp", SESSION);
    let environment = [("CLICOLOR_FORCE", "1")];
    let eval = sandbox.compiler_with_colors(&environment, &["eval", "session.rslisp", "("]);
    assert!(eval.stderr().contains("L0101"), "{}", eval.stderr());
    assert!(eval.stderr_is_colored(), "{}", eval.stderr());
}

#[test]
fn color_always_colors_diagnostics_in_a_pipe() {
    let sandbox = Sandbox::new("color_always").with_file("unbound.rslisp", UNBOUND_NAME);
    let check =
        sandbox.compiler_with_colors(&[], &["--color", "always", "check", "unbound.rslisp"]);
    assert!(
        check.assert_exit_code(1).stderr_is_colored(),
        "{}",
        check.stderr()
    );
}

#[test]
fn color_never_wins_over_clicolor_force_after_the_subcommand() {
    let sandbox = Sandbox::new("color_never").with_file("unbound.rslisp", UNBOUND_NAME);
    let environment = [("CLICOLOR_FORCE", "1")];
    let check =
        sandbox.compiler_with_colors(&environment, &["check", "unbound.rslisp", "--color=never"]);
    assert!(
        !check.assert_exit_code(1).stderr_is_colored(),
        "{}",
        check.stderr()
    );
}

#[test]
fn color_never_keeps_the_help_plain_before_the_subcommand() {
    let sandbox = Sandbox::new("color_never_help");
    let environment = [("CLICOLOR_FORCE", "1")];
    let help = sandbox.compiler_with_colors(&environment, &["--color", "never", "--help"]);
    assert!(
        !help.assert_exit_code(0).stdout_is_colored(),
        "{}",
        help.stdout()
    );
}

#[test]
fn color_never_keeps_the_help_plain_after_the_subcommand() {
    let sandbox = Sandbox::new("color_never_command_help");
    let environment = [("CLICOLOR_FORCE", "1")];
    let help = sandbox.compiler_with_colors(&environment, &["check", "--help", "--color=never"]);
    assert!(
        !help.assert_exit_code(0).stdout_is_colored(),
        "{}",
        help.stdout()
    );
}

#[test]
fn color_always_colors_the_help_after_the_subcommand() {
    let sandbox = Sandbox::new("color_always_command_help");
    let help = sandbox.compiler_with_colors(&[], &["check", "--help", "--color", "always"]);
    assert!(
        help.assert_exit_code(0).stdout_is_colored(),
        "{}",
        help.stdout()
    );
}

#[test]
fn color_never_keeps_a_usage_error_plain() {
    let sandbox = Sandbox::new("color_never_usage");
    let environment = [("CLICOLOR_FORCE", "1")];
    let usage = sandbox.compiler_with_colors(&environment, &["--color", "never", "nonsense"]);
    assert!(
        !usage.assert_exit_code(2).stderr_is_colored(),
        "{}",
        usage.stderr()
    );
}

#[test]
fn color_always_colors_a_usage_error() {
    let sandbox = Sandbox::new("color_always_usage");
    let usage = sandbox.compiler_with_colors(&[], &["--color", "always", "nonsense"]);
    assert!(
        usage.assert_exit_code(2).stderr_is_colored(),
        "{}",
        usage.stderr()
    );
}

#[test]
fn color_after_the_end_of_options_is_not_read() {
    let sandbox = Sandbox::new("color_after_end");
    let usage = sandbox.compiler_with_colors(&[], &["nonsense", "--", "--color", "always"]);
    assert!(
        !usage.assert_exit_code(2).stderr_is_colored(),
        "{}",
        usage.stderr()
    );
}

#[test]
fn color_with_an_unknown_value_is_a_usage_error_under_auto() {
    let sandbox = Sandbox::new("color_unknown").with_file("unbound.rslisp", UNBOUND_NAME);
    let environment = [("CLICOLOR_FORCE", "1")];
    let arguments = ["--color", "sometimes", "check", "unbound.rslisp"];
    let forced = sandbox.compiler_with_colors(&environment, &arguments);
    let piped = sandbox.compiler_with_colors(&[], &arguments);
    assert!(
        forced.assert_exit_code(2).stderr().contains("sometimes"),
        "{}",
        forced.stderr()
    );
    assert!(forced.stderr_is_colored(), "{}", forced.stderr());
    assert!(
        !piped.assert_exit_code(2).stderr_is_colored(),
        "{}",
        piped.stderr()
    );
}

#[test]
fn color_follows_the_last_color_argument() {
    let sandbox = Sandbox::new("color_last");
    let arguments = ["--color", "always", "check", "--help", "--color", "never"];
    let help = sandbox.compiler_with_colors(&[], &arguments);
    assert!(
        !help.assert_exit_code(0).stdout_is_colored(),
        "{}",
        help.stdout()
    );
}

#[test]
fn color_always_keeps_eval_json_plain() {
    let sandbox = Sandbox::new("color_json").with_file("session.rslisp", SESSION);
    let arguments = [
        "--color",
        "always",
        "eval",
        "--json",
        "session.rslisp",
        "(+ 1 \"b\")",
    ];
    let eval = sandbox.compiler_with_colors(&[], &arguments);
    let replies = eval.assert_exit_code(1).json_replies();
    let rendered = replies[0]["diagnostics"][0]["rendered"].as_str();
    assert!(
        rendered.is_some_and(|text| text.starts_with("error[L0401]")),
        "{rendered:?}"
    );
}

#[test]
fn eval_notes_a_struct_that_the_module_defines() {
    let sandbox = Sandbox::new("eval_struct_definition").with_file("points.rslisp", POINTS);
    let eval = sandbox.compiler(&[
        "eval",
        "--line",
        "1",
        "--column",
        "1",
        "points.rslisp",
        "(defstruct point :x i64 :y i64)",
    ]);
    assert_eq!(eval.assert_exit_code(0).stdout(), "");
}

#[test]
fn repl_keeps_definitions_and_prints_expressions() {
    let sandbox = Sandbox::new("repl_definitions");
    let repl = sandbox.compiler_reading(PROMPT_DEFINITIONS, &["repl"]);
    insta::assert_snapshot!(repl.transcript());
}

#[test]
fn repl_prints_a_doc_page_as_text() {
    let sandbox = Sandbox::new("repl_doc");
    let repl = sandbox.compiler_reading(PROMPT_DOC, &["repl"]);
    insta::assert_snapshot!(repl.transcript());
}

#[test]
fn repl_replaces_an_earlier_definition_of_the_same_name() {
    let sandbox = Sandbox::new("repl_redefinition");
    let repl = sandbox.compiler_reading(PROMPT_REDEFINITION, &["repl"]);
    insta::assert_snapshot!(repl.transcript());
}

#[test]
fn repl_rejects_a_replacement_that_would_drop_a_name() {
    let sandbox = Sandbox::new("repl_dropping_replacement");
    let repl = sandbox.compiler_reading(PROMPT_DROPPING_REPLACEMENT, &["repl"]);
    insta::assert_snapshot!(repl.transcript());
}

#[test]
fn repl_replaces_every_earlier_input_that_shares_a_name() {
    let sandbox = Sandbox::new("repl_replacement_of_two");
    let repl = sandbox.compiler_reading(PROMPT_REPLACEMENT_OF_TWO, &["repl"]);
    insta::assert_snapshot!(repl.transcript());
}

#[test]
fn repl_rejects_a_failing_definition_and_keeps_the_buffer() {
    let sandbox = Sandbox::new("repl_failing_definition");
    let repl = sandbox.compiler_reading(PROMPT_FAILING_DEFINITION, &["repl"]);
    insta::assert_snapshot!(repl.transcript());
}

#[test]
fn repl_enters_each_form_of_a_line_as_its_own_input() {
    let sandbox = Sandbox::new("repl_forms_on_one_line");
    let repl = sandbox.compiler_reading(PROMPT_FORMS_ON_ONE_LINE, &["repl"]);
    insta::assert_snapshot!(repl.transcript());
}

#[test]
fn repl_reads_an_unclosed_list_or_string_across_lines() {
    let sandbox = Sandbox::new("repl_multi_line");
    let repl = sandbox.compiler_reading(PROMPT_MULTI_LINE, &["repl"]);
    insta::assert_snapshot!(repl.transcript());
}

#[test]
fn repl_ends_an_input_at_an_extra_close() {
    let sandbox = Sandbox::new("repl_extra_close");
    let repl = sandbox.compiler_reading(PROMPT_EXTRA_CLOSE, &["repl"]);
    insta::assert_snapshot!(repl.transcript());
}

#[test]
fn repl_resolves_use_from_the_root_and_replaces_by_the_same_rule() {
    let sandbox = Sandbox::new("repl_uses").with_file("lib/shapes.rslisp", SHAPES_OF_TWO);
    let repl = sandbox.compiler_reading(PROMPT_USES, &["repl", "--root", "lib"]);
    insta::assert_snapshot!(repl.transcript());
}

#[test]
fn repl_shows_a_panic_and_keeps_its_definitions() {
    let sandbox = Sandbox::new("repl_panic");
    let repl = sandbox.compiler_reading(PROMPT_PANIC, &["repl"]);
    let stderr = repl.assert_exit_code(0).stderr();
    assert!(stderr.contains("index out of bounds"), "{stderr}");
    assert!(
        stderr.ends_with("note: the program exited with code 101\n"),
        "{stderr}"
    );
    assert_eq!(repl.stdout(), "8\n");
}

#[test]
fn repl_notes_a_nonzero_exit_of_a_program() {
    let sandbox = Sandbox::new("repl_exit");
    let repl = sandbox.compiler_reading(PROMPT_EXIT, &["repl"]);
    insta::assert_snapshot!(repl.transcript());
}

#[test]
fn repl_reports_the_end_of_input_inside_a_list_and_quits() {
    let sandbox = Sandbox::new("repl_unclosed_end");
    let repl = sandbox.compiler_reading(PROMPT_UNCLOSED_END, &["repl"]);
    insta::assert_snapshot!(repl.transcript());
}

#[test]
fn repl_replaces_an_enum_by_every_name_it_defines() {
    let sandbox = Sandbox::new("repl_enum_redefinition");
    let repl = sandbox.compiler_reading(PROMPT_ENUM_REDEFINITION, &["repl"]);
    insta::assert_snapshot!(repl.transcript());
}

#[test]
fn repl_prints_a_warning_of_a_definition_once() {
    let sandbox = Sandbox::new("repl_warning");
    let repl = sandbox.compiler_reading(PROMPT_WARNING, &["repl"]);
    insta::assert_snapshot!(repl.transcript());
}

#[test]
fn repl_reports_unreadable_input() {
    let sandbox = Sandbox::new("repl_unreadable_input");
    let repl = sandbox.compiler_reading(PROMPT_NOT_UTF8, &["repl"]);
    assert_eq!(repl.assert_exit_code(2).stdout(), "2\n");
    assert_eq!(
        repl.stderr(),
        "error: cannot read standard input: stream did not contain valid UTF-8\n"
    );
}

#[test]
fn eval_reports_unreadable_input() {
    let sandbox = Sandbox::new("eval_unreadable_input");
    let eval = sandbox.compiler_reading(b"\xff", &["eval", "--stdin", "buffer.rslisp", "1"]);
    assert_eq!(eval.assert_exit_code(2).stdout(), "");
    assert_eq!(
        eval.stderr(),
        "error: cannot read standard input: stream did not contain valid UTF-8\n"
    );
}

struct PromptColors;

impl PromptColors {
    const EXIT_NOTE: &str = "the program exited with code 3";

    fn prompt() -> String {
        let style = DiagnosticRenderer::GUTTER_STYLE;
        format!("{style}> {style:#}")
    }

    fn exit_note() -> String {
        DiagnosticRenderer::styled(&SourceMap::default()).render_note(Self::EXIT_NOTE)
    }

    fn plain_exit_note() -> String {
        format!("note: {}", Self::EXIT_NOTE)
    }

    fn session(name: &str, environment: &[(&str, &str)], arguments: &[&str]) -> String {
        let sandbox = Sandbox::new(name);
        sandbox
            .compiler_on_a_terminal_reading(PROMPT_EXIT_ON_A_TERMINAL, environment, arguments)
            .assert_exit_code(0)
            .stdout()
    }
}

#[test]
fn color_repl_prompt_and_exit_note_on_a_terminal() {
    let session = PromptColors::session("color_repl", &[("TERM", "xterm")], &["repl"]);
    assert!(session.contains(&PromptColors::prompt()), "{session:?}");
    assert!(session.contains(&PromptColors::exit_note()), "{session:?}");
}

#[test]
fn color_stays_off_the_repl_prompt_and_exit_note_when_no_color_is_set() {
    let session = PromptColors::session(
        "color_repl_no_color",
        &[("TERM", "xterm"), ("NO_COLOR", "1")],
        &["repl"],
    );
    assert!(!session.contains('\u{1b}'), "{session:?}");
    assert!(session.contains("> "), "{session:?}");
    assert!(
        session.contains(&PromptColors::plain_exit_note()),
        "{session:?}"
    );
}

#[test]
fn color_never_keeps_the_repl_prompt_and_exit_note_plain() {
    let session = PromptColors::session(
        "color_repl_never",
        &[("TERM", "xterm")],
        &["--color", "never", "repl"],
    );
    assert!(!session.contains('\u{1b}'), "{session:?}");
    assert!(session.contains("> "), "{session:?}");
    assert!(
        session.contains(&PromptColors::plain_exit_note()),
        "{session:?}"
    );
}
