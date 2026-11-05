use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use rslisp::build::EmittedFormatting;
use rslisp::diagnostic::DiagnosticRenderer;
use rslisp::language::Language;
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
const INTERRUPTED_RUSTC: &str = "#!/bin/sh\nkill -INT $$\n";
const WAITING_RUSTC: &str = "#!/bin/sh\necho compiling > /dev/tty\nexec sleep 30\n";
const WAITING_RUSTFMT: &str = "#!/bin/sh\necho formatting > /dev/tty\nexec sleep 30\n";
const SIGNAL_CATCHING_RUSTC: &str = "#!/bin/sh\nsleep 30 > /dev/null 2>&1 &\nsleeper=$!\ntrap 'kill $sleeper; echo \"error: compilation was interrupted\" >&2; exit 1' INT\necho compiling > /dev/tty\nwait\n";
const SIGNAL_CATCHING_RUSTFMT: &str = "#!/bin/sh\nsleep 30 > /dev/null 2>&1 &\nsleeper=$!\ntrap 'kill $sleeper; exit 1' INT\necho formatting > /dev/tty\nwait\n";
const INTERRUPTED_RUSTFMT: &str = "#!/bin/sh\nkill -INT $$\n";
const UNINTERRUPTED_RUSTFMT: &str =
    "#!/bin/sh\ntrap '' INT\necho formatting > /dev/tty\nsleep 1\nexec cat\n";
const PASSTHROUGH_RUSTFMT: &str =
    "#!/bin/sh\nwhile IFS= read -r line; do printf '%s\\n' \"$line\"; done\n";
const FAILING_RUSTFMT: &str = "#!/bin/sh\nprintf 'rustfmt failed in the sandbox\\n' >&2\nexit 1\n";
const BOUND_MODULE: &str = "(export person)\n\n(defstruct person :name string)\n";
const MILLION_LINES: &str = "(defn main ()\n  (for-each (range 0 1000000) (fn (i) (println i))))\n";
const THOUSAND_LINES: &str = "(defn main ()\n  (for-each (range 0 1000) (fn (i) (println i))))\n";
const ABORT: &str =
    "(def abort (rust \"std::process::abort()\" (fn () unit)))\n\n(defn main ()\n  (abort))\n";
const PROMPT_THEN_READ_LINE: &str = "(defn main ()\n  (print \"who? \")\n  (prn (read-line)))\n";
const PROMPT_THEN_SLURP: &str = "(defn main ()\n  (print \"who? \")\n  (prn (slurp stdin)))\n";
const PROMPT_THEN_SLURP_ANY_SOURCE: &str = "(type load (where ((s source)) (fn ((ref s)) (result string io-error))))\n(defn load (input) (slurp input))\n\n(defn main ()\n  (print \"who? \")\n  (prn (load stdin)))\n";
const COUNT_INPUT_LINES: &str = "(defn main ()\n  (println (count (take (line-seq stdin) 2))))\n";
const STRUCTS: &str = "(defstruct person :name string :age i64)\n(defstruct ticket :id u32)\n(derive ticket debug)\n\n(defn main () (do))\n";
const BOUND_ENTRY: &str = "(use other/person person)\n\n(rust-trait same \"std::ops::Add\" i64 (with i64 person))\n\n(type matches (where ((a (same person))) (fn (a) bool)))\n(defn matches (x) true)\n\n(defn main () (println (matches 1)))\n";

const TRAIT_SHARED_VECTOR: &str = "(struct odometer (offset i64))\n\n(trait mileage\n  (first-reading (fn ((ref self) (ref (vec i64))) i64)))\n\n(impl mileage odometer\n  (first-reading (o readings) (+ (field o offset) (at readings 0))))\n\n(defn main ()\n  (println (first-reading (new odometer (offset 10)) (vec 1200))))\n";
const GEOMETRY: &str = "(export area)\n\n(defn area (width height) \"Multiplies a width by a height.\" (* width height))\n";
const GEOMETRY_ENTRY: &str = "(use geometry area)\n\n(defn main () (println (area 3 4)))\n";
const UNDOCUMENTED_ENTRY: &str = "(defn twice (x) (* 2 x))\n\n(defn main () (println (twice 2)))\n";
const SHADOWING_PAGE_BODY: &str = "(defrule page-body ((term) 0))\n\n(defn main () (do))\n";
const SHADOWING_HEADER: &str = "(defrule header ((term) 0))\n\n(defn main () (do))\n";
const IF_PAGE: &str = "(if test then else)\n\nEvaluates `then` when the test is true, and `else` otherwise.\n\nIt emits a Rust `if` with an `else`.\n";
const MACRO_STRUCT_BESIDE_A_DEF: &str = "(defrule my-struct ((n) (do (defstruct n \"A documented struct.\" :x i64))))\n(my-struct point)\n(def point 1)\n(defn main () (do))\n";
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
const PROMPT_DROPPING_REPLACEMENT: &str = "(defstruct point :x i64 :y i64)
(defn ->point (x y) (vec x y))
(->point 1 2)
";
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
const PROMPT_REDEFINED_STRUCT: &str = "(defstruct user :name str)\n(def karl (->user :name \"karl\"))\n(defstruct user :name str :email str)\n(karl)\n";
const PROMPT_FAILED_REDEFINED_STRUCT: &str = "(defstruct user :name str)\n(def karl (->user :name \"karl\"))\n(defstruct user (name str))\n(:name karl)\n";
const PROMPT_CASCADE: &str = "(defstruct user :name str)\n(def karl (->user :name \"karl\"))\n(defn greet () (str \"hi \" (:name karl)))\n(defstruct user :name str :email str)\n(greet)\n";
const PROMPT_BROKEN_MACRO_CALLED: &str =
    "(defmacro bad () (template (missing-helper 1)))\n(+ 1 2)\n(defn f () (bad))\n(f)\n";
const PROMPT_BROKEN_MACRO_REDEFINED: &str = "(defmacro bad () (template 1))\n(defn f () (bad))\n(defmacro bad () (template (missing-helpr 1)))\n(f)\n";
const PROMPT_STOPS_BINDING: &str = "(defn pick (a b) a)\n(defmacro mk (a b) (template (defn (unquote (pick a b)) () 1)))\n(mk foo bar)\n(defn pick (a b) b)\n(foo)\n(bar)\n";
const PROMPT_TWO_BINDERS: &str = "(defn pick (a b) a)\n(defmacro mk (a b) (template (defn (unquote (pick a b)) () 1)))\n(mk foo bar)\n(defn bar () 2)\n(defn pick (a b) b)\n(bar)\n";
const PROMPT_WOULD_SET_ASIDE: &str = "(defstruct user :name str)\n(def karl (->user :name \"karl\"))\n(do (defstruct user :name str :email str) (defn show () (:name karl)))\n(:name karl)\n";
const PROMPT_RE_ENTERED_TYPE: &str = "(defstruct point :x i64 :y i64)\n(defn ->point (x y) (vec x y))\n(defstruct point :x i64 :y i64)\n(use geo area)\n";
const GEO_WITHOUT_POINT: &str = "(export area)\n(defn area (:p point) 1)\n";
const PROMPT_EXPRESSION_CALLING_A_BROKEN_MACRO: &str =
    "(defmacro bad () (template (missing-helper 1)))\n(bad)\n";
const PROMPT_TWO_ERRORS_IN_ONE_INPUT: &str = "(defstruct user :name str)\n(def karl (->user :name \"karl\"))\n(defn greet () (str (:name karl) (:name karl)))\n(defstruct user :name str :email str)\n";
const SHAPES_WITH_A_TYPO: &str =
    "(write-file \"shapes.rslisp\" \"(export circle)\\n(defn circle (r) (nope r))\\n\")\n";
const WRITE_FILE: &str = "(def write-file (rust \"std::fs::write({0}, {1}).unwrap()\" (fn ((ref str) (ref str)) unit)))\n";
const SHAPES_WITHOUT_SQUARE: &str =
    "(write-file \"shapes.rslisp\" \"(export circle)\\n(defn circle (r) r)\\n\")\n";
const PROMPT_EXIT_ON_A_TERMINAL: &[Keys] = &[
    Keys::AtPrompt("(def exit (rust \"std::process::exit({0})\" (fn (i32) unit)))\r"),
    Keys::AtPrompt("(exit 3)\r"),
    Keys::AtPrompt(Keys::END_OF_INPUT),
];
const PROMPT_EXTRA_CLOSE: &str = "(+ 1 2))\n(+ 3 4)\n(+ 3 \"a\")\n";
const PROMPT_ENUM_REDEFINITION: &str = "(enum shape circle square)\n(enum shape circle square triangle)\n(match shape/triangle (triangle 3) (_ 0))\n";
const PROMPT_WARNING: &str = "(defn pick (x) (match x (_ 1) (_ 2)))\n(pick 1)\n(defn other () 1)\n(defn another () 2)\n(pick 2)\n";

const SHADOWED_PRINTLN: &str = "(defn println (x) x)\n(defn square (n) (* n n))\n";
const COUNT_THEN_FAIL: &str = "(def exit (rust \"std::process::exit({0})\" (fn (i32) unit)))\n(type count-up (fn (i64) unit))\n(defn count-up (i)\n  (println i)\n  (if (< i 1000000) (recur (+ i 1)) (exit 7)))\n";

const UNFORMATTED: &str = "(defn twice (x)\n      (* 2 x))   ; doubles\n(def limit 10)\n";
const FORMATTED: &str = "(defn twice (x) (* 2 x)) ; doubles\n\n(def limit 10)\n";
const UNCLOSED: &str = "(defn main ()\n  (println 1)\n";
const LOOP_SHAPE: &str = "(use prelude/macros header-shape)\n\n(export dotimes)\n\n(defrule dotimes ((binding body ...) (do body ...)))\n\n(header-shape (dotimes binding))\n";
const LOOP_CALL: &str = "(use shapes/loops dotimes)\n\n(dotimes (index 10) (println \"first line of the body\") (println index))\n";
const LOOP_CALL_SHAPED: &str = "(use shapes/loops dotimes)\n\n(dotimes (index 10)\n  (println \"first line of the body\")\n  (println index))\n";

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
    const PIPE_OVERFLOWING_GREETING_REPEATS: usize = 1 << 18;
    const READ_ONLY: u32 = 0o444;
    const TERMINAL_RECORDER: &str = "script";
    const INPUT_PATIENCE: Duration = Duration::from_secs(10);
    const STATE_HOME_VARIABLE: &str = "XDG_STATE_HOME";
    const STATE_DIRECTORY: &str = "state";
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

    fn contents(&self, name: &str) -> String {
        fs::read_to_string(self.directory.join(name)).expect("the sandbox file is readable")
    }

    fn with_unbound_names(self, name: &str, count: usize) -> Self {
        let body: String = (0..count)
            .map(|index| format!("    missing{index}\n"))
            .collect();
        self.with_file(name, &format!("(defn main ()\n  (do\n{body}    1))\n"))
    }

    fn pipe_overflowing_greeting() -> String {
        let greeting = "hello ".repeat(Self::PIPE_OVERFLOWING_GREETING_REPEATS);
        format!("(defn main () (println \"{greeting}\"))\n")
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
        typed: &[Keys],
        environment: &[(&str, &str)],
        arguments: &[&str],
    ) -> Outcome {
        let recorded = Self::recorded_arguments(arguments);
        let recorded: Vec<&str> = recorded.iter().map(String::as_str).collect();
        let state = self.directory.join(Self::STATE_DIRECTORY);
        let state = state.to_str().expect("the sandbox path is UTF-8");
        let environment: Vec<(&str, &str)> = [(Self::STATE_HOME_VARIABLE, state)]
            .into_iter()
            .chain(environment.iter().copied())
            .collect();
        let mut command =
            self.color_command(Path::new(Self::TERMINAL_RECORDER), &environment, &recorded);
        Self::typing_until_the_end(&mut command, typed)
    }

    fn typing_until_the_end(command: &mut Command, typed: &[Keys]) -> Outcome {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the terminal recorder starts");
        let mut keyboard = child.stdin.take().expect("stdin is piped");
        let mut stdout = child.stdout.take().expect("stdout is piped");
        let mut stderr = child.stderr.take().expect("stderr is piped");
        let (stdout, stderr, status) = std::thread::scope(|scope| {
            let (sender, updates) = mpsc::channel();
            let stdout = scope.spawn(move || TerminalScreen::forward(&mut stdout, &sender));
            let stderr = scope.spawn(move || Self::read_all(&mut stderr));
            let mut screen = TerminalScreen::new(updates);
            for keys in typed {
                if let Err(missing) = screen.type_keys(keys, &mut keyboard) {
                    child.kill().ok();
                    panic!("{missing}");
                }
            }
            let status = child.wait().expect("the terminal recorder finishes");
            (
                stdout.join().expect("stdout is read"),
                stderr.join().expect("stderr is read"),
                status,
            )
        });
        drop(keyboard);
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
        self.compiler_writing_to(Self::closed_pipe(), arguments)
    }

    fn compiler_with_a_closed_standard_error(&self, arguments: &[&str]) -> Outcome {
        self.compiler_reporting_to(Self::closed_pipe(), arguments)
    }

    fn assert_exit_code_with_a_closed_error_pipe(&self, arguments: &[&str], expected: i32) {
        self.compiler(arguments).assert_exit_code(expected);
        self.compiler_with_a_closed_standard_error(arguments)
            .assert_exit_code(expected);
    }

    fn closed_pipe() -> Stdio {
        let (reader, writer) = std::io::pipe().expect("the test opens a pipe");
        drop(reader);
        Stdio::from(writer)
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

    fn compiler_reporting_to(&self, standard_error: Stdio, arguments: &[&str]) -> Outcome {
        let mut command = self.command(Path::new(Self::COMPILER), arguments);
        command.stderr(standard_error);
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

    fn built_program_output_before_its_input(&self, relative_path: &str, input: &str) -> String {
        let mut child = self
            .command(&self.directory.join(relative_path), &[])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("the built program starts");
        let mut stdout = child.stdout.take().expect("stdout is piped");
        let (sender, shown) = mpsc::channel();
        let reader = std::thread::spawn(move || TerminalScreen::forward(&mut stdout, &sender));
        let before_input = shown.recv_timeout(Self::INPUT_PATIENCE).unwrap_or_default();
        std::io::Write::write_all(
            &mut child.stdin.take().expect("stdin is piped"),
            input.as_bytes(),
        )
        .expect("the program reads its input");
        child.wait().expect("the program finishes");
        reader.join().expect("stdout is read");
        String::from_utf8(before_input).expect("the program prints UTF-8")
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

enum Keys<'a> {
    AtPrompt(&'a str),
    AtPromptAfter(&'a str, &'a str),
    AfterOutput(&'a str, &'a str),
    BytesAtPrompt(&'a [u8]),
}

impl Keys<'_> {
    const END_OF_INPUT: &'static str = "\u{4}";
    const INTERRUPT: &'static str = "\u{3}";
    const LEFT: &'static str = "\u{1b}[D";
    const PASTE_START: &'static str = "\u{1b}[200~";
    const PASTE_END: &'static str = "\u{1b}[201~";
}

struct RenderedScreen {
    rows: Vec<Vec<char>>,
    row: usize,
    column: usize,
}

impl RenderedScreen {
    const ESCAPE: char = '\u{1b}';

    fn of(session: &str) -> String {
        let mut screen = Self {
            rows: vec![Vec::new()],
            row: 0,
            column: 0,
        };
        let mut characters = session.chars();
        while let Some(character) = characters.next() {
            screen.apply(character, &mut characters);
        }
        screen.text()
    }

    fn apply(&mut self, character: char, rest: &mut std::str::Chars) {
        match character {
            Self::ESCAPE => self.apply_sequence(rest),
            '\r' => self.column = 0,
            '\n' => self.move_down(),
            _ => self.write(character),
        }
    }

    fn apply_sequence(&mut self, rest: &mut std::str::Chars) {
        rest.next();
        let mut parameters = String::new();
        for character in rest.by_ref() {
            if character.is_ascii_alphabetic() {
                return self.apply_final(character, parameters.parse().unwrap_or(1));
            }
            parameters.push(character);
        }
    }

    fn apply_final(&mut self, last: char, count: usize) {
        match last {
            'A' => self.row = self.row.saturating_sub(count),
            'C' => self.column += count,
            'D' => self.column = self.column.saturating_sub(count),
            'K' => self.rows[self.row].truncate(self.column),
            _ => {}
        }
    }

    fn move_down(&mut self) {
        self.row += 1;
        if self.row == self.rows.len() {
            self.rows.push(Vec::new());
        }
    }

    fn write(&mut self, character: char) {
        let row = &mut self.rows[self.row];
        if row.len() <= self.column {
            row.resize(self.column, ' ');
            row.push(character);
        } else {
            row[self.column] = character;
        }
        self.column += 1;
    }

    fn text(&self) -> String {
        let rows: Vec<String> = self.rows.iter().map(|row| row.iter().collect()).collect();
        rows.join("\n")
    }
}

struct TerminalScreen {
    updates: Receiver<Vec<u8>>,
    shown: Vec<u8>,
    seen: usize,
}

impl TerminalScreen {
    const PROMPT_READY: &str = "\u{1b}[?2004h";
    const PATIENCE: Duration = Duration::from_secs(60);
    const CHUNK: usize = 4096;

    fn new(updates: Receiver<Vec<u8>>) -> Self {
        Self {
            updates,
            shown: Vec::new(),
            seen: 0,
        }
    }

    fn forward(stream: &mut impl std::io::Read, sender: &Sender<Vec<u8>>) -> Vec<u8> {
        let mut everything = Vec::new();
        let mut chunk = [0; Self::CHUNK];
        while let Ok(read @ 1..) = stream.read(&mut chunk) {
            everything.extend_from_slice(&chunk[..read]);
            sender.send(chunk[..read].to_vec()).ok();
        }
        everything
    }

    fn type_keys(&mut self, keys: &Keys, keyboard: &mut impl std::io::Write) -> Result<(), String> {
        let keys = match keys {
            Keys::AtPrompt(keys) => keys.as_bytes(),
            Keys::AtPromptAfter(output, keys) => {
                self.wait_for(output)?;
                keys.as_bytes()
            }
            Keys::AfterOutput(output, keys) => {
                self.wait_for(output)?;
                Self::press(keys.as_bytes(), keyboard);
                return Ok(());
            }
            Keys::BytesAtPrompt(bytes) => bytes,
        };
        self.wait_for(Self::PROMPT_READY)?;
        Self::press(keys, keyboard);
        Ok(())
    }

    fn press(keys: &[u8], keyboard: &mut impl std::io::Write) {
        keyboard
            .write_all(keys)
            .and_then(|()| keyboard.flush())
            .expect("the terminal recorder reads its input");
    }

    fn wait_for(&mut self, awaited: &str) -> Result<(), String> {
        while self.end_of_next(awaited).is_none() {
            let update = self.updates.recv_timeout(Self::PATIENCE).map_err(|_| {
                format!(
                    "the terminal never showed {awaited:?}: {:?}",
                    String::from_utf8_lossy(&self.shown)
                )
            })?;
            self.shown.extend(update);
        }
        self.seen = self
            .end_of_next(awaited)
            .expect("the awaited text was shown");
        Ok(())
    }

    fn end_of_next(&self, awaited: &str) -> Option<usize> {
        let awaited = awaited.as_bytes();
        self.shown[self.seen..]
            .windows(awaited.len())
            .position(|window| window == awaited)
            .map(|start| self.seen + start + awaited.len())
    }
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
fn no_command_runs_piped_inputs_as_repl_does() {
    let sandbox = Sandbox::new("no_command");
    let bare = sandbox.compiler_reading(PROMPT_DEFINITIONS, &[]);
    let repl = sandbox.compiler_reading(PROMPT_DEFINITIONS, &["repl"]);
    assert_eq!(bare.transcript(), repl.transcript());
}

#[test]
fn no_command_takes_the_global_color_argument() {
    let sandbox = Sandbox::new("no_command_color");
    let bare = sandbox.compiler_reading(PROMPT_DEFINITIONS, &["--color", "never"]);
    let repl = sandbox.compiler_reading(PROMPT_DEFINITIONS, &["repl"]);
    assert_eq!(bare.transcript(), repl.transcript());
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
fn fmt_rewrites_a_file_in_place_and_prints_nothing() {
    let sandbox = Sandbox::new("fmt_rewrites").with_file("twice.rslisp", UNFORMATTED);
    let formatted = sandbox.compiler(&["fmt", "twice.rslisp"]);
    formatted.assert_silent_success();
    assert_eq!(
        (formatted.stdout(), sandbox.contents("twice.rslisp")),
        (String::new(), FORMATTED.to_string())
    );
}

#[test]
fn fmt_check_prints_each_file_that_would_change_and_writes_nothing() {
    let sandbox = Sandbox::new("fmt_check_changes")
        .with_file("twice.rslisp", UNFORMATTED)
        .with_file("done.rslisp", FORMATTED);
    let check = sandbox.compiler(&["fmt", "--check", "twice.rslisp", "done.rslisp"]);
    check.assert_exit_code(1);
    assert_eq!(
        (check.stdout(), sandbox.contents("twice.rslisp")),
        ("twice.rslisp\n".to_string(), UNFORMATTED.to_string())
    );
}

#[test]
fn fmt_check_exits_0_when_every_file_is_formatted() {
    let sandbox = Sandbox::new("fmt_check_formatted").with_file("done.rslisp", FORMATTED);
    let check = sandbox.compiler(&["fmt", "--check", "done.rslisp"]);
    check.assert_silent_success();
    assert_eq!(check.stdout(), "");
}

#[test]
fn fmt_reads_standard_input_and_writes_the_formatted_text() {
    let sandbox = Sandbox::new("fmt_standard_input");
    let formatted = sandbox.compiler_reading(UNFORMATTED, &["fmt", "-"]);
    formatted.assert_silent_success();
    assert_eq!(formatted.stdout(), FORMATTED);
}

#[test]
fn fmt_of_standard_input_reads_shapes_from_modules_beside_its_stdin_path() {
    let sandbox =
        Sandbox::new("fmt_standard_input_shapes").with_file("shapes/loops.rslisp", LOOP_SHAPE);
    let formatted = sandbox.compiler_reading(
        LOOP_CALL,
        &["fmt", "--stdin-path", "programs/main.rslisp", "-"],
    );
    formatted.assert_silent_success();
    assert_eq!(formatted.stdout(), LOOP_CALL_SHAPED);
}

#[test]
fn fmt_formats_every_source_file_under_a_directory() {
    let sandbox = Sandbox::new("fmt_directory")
        .with_file("project/twice.rslisp", UNFORMATTED)
        .with_file("project/nested/limit.rslisp", UNFORMATTED)
        .with_file("project/notes.txt", UNFORMATTED);
    sandbox
        .compiler(&["fmt", "project"])
        .assert_silent_success();
    assert_eq!(
        [
            sandbox.contents("project/twice.rslisp"),
            sandbox.contents("project/nested/limit.rslisp"),
            sandbox.contents("project/notes.txt"),
        ],
        [FORMATTED, FORMATTED, UNFORMATTED].map(String::from)
    );
}

#[test]
fn fmt_check_of_standard_input_that_would_change_prints_its_name_and_exits_1() {
    let sandbox = Sandbox::new("fmt_check_standard_input_changes");
    let check = sandbox.compiler_reading(UNFORMATTED, &["fmt", "--check", "-"]);
    check.assert_exit_code(1);
    assert_eq!(check.stdout(), "<stdin>\n");
}

#[test]
fn fmt_check_of_formatted_standard_input_prints_nothing() {
    let sandbox = Sandbox::new("fmt_check_standard_input_formatted");
    let check = sandbox.compiler_reading(FORMATTED, &["fmt", "--check", "-"]);
    check.assert_silent_success();
    assert_eq!(check.stdout(), "");
}

#[test]
fn fmt_exits_2_when_an_unreadable_file_follows_a_reader_error() {
    let sandbox =
        Sandbox::new("fmt_unreadable_after_reader_error").with_file("unclosed.rslisp", UNCLOSED);
    sandbox
        .compiler(&["fmt", "unclosed.rslisp", "missing.rslisp"])
        .assert_exit_code(2);
}

#[test]
fn fmt_skips_a_symlinked_directory() {
    let sandbox = Sandbox::new("fmt_symlinked_directory")
        .with_file("project/twice.rslisp", UNFORMATTED)
        .with_file("elsewhere/limit.rslisp", UNFORMATTED);
    std::os::unix::fs::symlink(
        sandbox.directory.join("elsewhere"),
        sandbox.directory.join("project/linked"),
    )
    .expect("the sandbox accepts a symlink");
    sandbox
        .compiler(&["fmt", "project"])
        .assert_silent_success();
    assert_eq!(
        [
            sandbox.contents("project/twice.rslisp"),
            sandbox.contents("elsewhere/limit.rslisp"),
        ],
        [FORMATTED, UNFORMATTED].map(String::from)
    );
}

#[test]
fn fmt_reports_a_reader_error_and_leaves_the_file_as_it_was() {
    let sandbox = Sandbox::new("fmt_reader_error")
        .with_file("unclosed.rslisp", UNCLOSED)
        .with_file("twice.rslisp", UNFORMATTED);
    let formatted = sandbox.compiler(&["fmt", "unclosed.rslisp", "twice.rslisp"]);
    formatted.assert_exit_code(1);
    assert!(
        formatted.stderr().contains("error[L0101]"),
        "{}",
        formatted.stderr()
    );
    assert_eq!(
        (
            sandbox.contents("unclosed.rslisp"),
            sandbox.contents("twice.rslisp")
        ),
        (UNCLOSED.to_string(), FORMATTED.to_string())
    );
}

#[test]
fn fmt_of_a_missing_file_is_a_usage_error() {
    let sandbox = Sandbox::new("fmt_missing_file");
    let formatted = sandbox.compiler(&["fmt", "missing.rslisp"]);
    assert!(
        formatted
            .assert_exit_code(2)
            .stderr()
            .starts_with("error: cannot read `missing.rslisp`")
    );
}

#[test]
fn fmt_without_a_path_is_a_usage_error() {
    Sandbox::new("fmt_without_path")
        .compiler(&["fmt"])
        .assert_exit_code(2);
}

#[test]
fn fmt_of_standard_input_with_another_path_is_a_usage_error() {
    let sandbox = Sandbox::new("fmt_standard_input_and_path").with_file("done.rslisp", FORMATTED);
    let formatted = sandbox.compiler_reading(FORMATTED, &["fmt", "-", "done.rslisp"]);
    assert!(
        formatted
            .assert_exit_code(2)
            .stderr()
            .starts_with("error: `-` reads standard input")
    );
}

#[test]
fn fmt_with_a_stdin_path_but_no_standard_input_is_a_usage_error() {
    let sandbox = Sandbox::new("fmt_stdin_path_alone").with_file("done.rslisp", FORMATTED);
    sandbox
        .compiler(&["fmt", "--stdin-path", "done.rslisp", "done.rslisp"])
        .assert_exit_code(2);
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
fn doc_keeps_its_exit_code_when_its_error_reader_closes() {
    let sandbox = Sandbox::new("doc_closed_error_pipe").with_file("unbound.rslisp", UNBOUND_NAME);
    let arguments = ["doc", "--file", "unbound.rslisp", "main"];
    sandbox.assert_exit_code_with_a_closed_error_pipe(&arguments, 1);
}

#[test]
fn check_keeps_its_exit_code_when_its_error_reader_closes() {
    let sandbox = Sandbox::new("check_closed_error_pipe").with_file("unbound.rslisp", UNBOUND_NAME);
    let arguments = ["check", "unbound.rslisp"];
    sandbox.assert_exit_code_with_a_closed_error_pipe(&arguments, 1);
}

#[test]
fn run_keeps_its_exit_code_when_its_error_reader_closes() {
    let sandbox = Sandbox::new("run_closed_error_pipe").with_file("unbound.rslisp", UNBOUND_NAME);
    let arguments = ["run", "unbound.rslisp"];
    sandbox.assert_exit_code_with_a_closed_error_pipe(&arguments, 1);
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
fn doc_prints_the_header_that_documents_a_prelude_term() {
    let sandbox = Sandbox::new("doc_document_header");
    let doc = sandbox.compiler(&["doc", "str"]);
    assert_eq!(
        doc.assert_exit_code(0).stdout(),
        "(str part ...)\n\nBuilds a `string` from its parts.\n\nA string literal part is literal text, and any other part prints with\n`display`. `(str x)` emits `x.to_string()`, and `str` as a value is that\none-part function, so `(map (iter years) str)` turns each year into a\n`string`.\n"
    );
}

#[test]
fn doc_prints_the_page_of_a_kernel_form() {
    let sandbox = Sandbox::new("doc_kernel_form");
    let doc = sandbox.compiler(&["doc", "match"]);
    assert_eq!(
        doc.assert_exit_code(0).stdout(),
        "(match value (pattern body) ...)\n\nEvaluates the body of the first arm whose pattern matches the value.\n\nA pattern is `_`, a literal, a bare name, `(variant pattern ...)` or\n`(type-name pattern ...)`. It emits a Rust `match`.\n"
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
fn doc_rejects_a_shadowing_page_body_that_returns_no_string() {
    let sandbox = Sandbox::new("doc_shadowed").with_file("main.rslisp", SHADOWING_PAGE_BODY);
    let doc = sandbox.compiler(&["doc", "--file", "main.rslisp", "main"]);
    assert_eq!(
        doc.assert_exit_code(1).stderr(),
        "error: `page-body` did not expand to a string\n"
    );
}

#[test]
fn doc_rejects_a_shadowing_header_that_returns_no_string() {
    let sandbox = Sandbox::new("doc_shadowed_header").with_file("main.rslisp", SHADOWING_HEADER);
    let doc = sandbox.compiler(&["doc", "--file", "main.rslisp", "main"]);
    assert_eq!(
        doc.assert_exit_code(1).stderr(),
        "error: `header` did not expand to a string\n"
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
    assert_eq!(sandbox.typed_page(&["if"]), IF_PAGE);
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
        "(type limit i64)\n(def limit)\n\nNo documentation.\n"
    );
}

#[test]
fn doc_types_prints_no_type_for_a_value_that_no_definition_binds() {
    let sandbox = Sandbox::new("doc_types_literal");
    assert_eq!(sandbox.typed_page(&["true"]), "true\n\nNo documentation.\n");
}

#[test]
fn doc_types_prints_no_type_above_a_struct_page_whose_header_matches_the_value() {
    let sandbox =
        Sandbox::new("doc_types_same_header").with_file("prog.rslisp", MACRO_STRUCT_BESIDE_A_DEF);
    assert_eq!(
        sandbox.typed_page(&["--file", "prog.rslisp", "point"]),
        "point\n\nA documented struct.\n"
    );
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
        "(type point i64)\n(def point)\n\nNo documentation.\n"
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
        "(type page (ref str))\n(def page)\n\nNo documentation.\n"
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
fn built_program_shows_a_prompt_before_it_reads_a_line() {
    let sandbox = Sandbox::new("prompt_before_read").with_file("who.rslisp", PROMPT_THEN_READ_LINE);
    sandbox
        .compiler(&["build", "who.rslisp"])
        .assert_exit_code(0);
    let shown = sandbox.built_program_output_before_its_input("target/rslisp/who/who", "Ada\n");
    assert_eq!(shown, "who? ");
}

#[test]
fn built_program_shows_a_prompt_before_it_slurps_its_input() {
    let sandbox = Sandbox::new("prompt_before_slurp").with_file("who.rslisp", PROMPT_THEN_SLURP);
    sandbox
        .compiler(&["build", "who.rslisp"])
        .assert_exit_code(0);
    let shown = sandbox.built_program_output_before_its_input("target/rslisp/who/who", "Ada\n");
    assert_eq!(shown, "who? ");
}

#[test]
fn built_program_shows_a_prompt_before_it_slurps_any_source() {
    let sandbox =
        Sandbox::new("prompt_before_source").with_file("who.rslisp", PROMPT_THEN_SLURP_ANY_SOURCE);
    sandbox
        .compiler(&["build", "who.rslisp"])
        .assert_exit_code(0);
    let shown = sandbox.built_program_output_before_its_input("target/rslisp/who/who", "Ada\n");
    assert_eq!(shown, "who? ");
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
fn repl_sets_aside_a_name_that_a_replacement_drops() {
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
fn repl_resolves_use_from_the_root_and_sets_aside_an_import_it_drops() {
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
    const CONTROL_SEQUENCE: &str = "\u{1b}[";

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

    fn is_styled(session: &str) -> bool {
        session
            .split(Self::CONTROL_SEQUENCE)
            .skip(1)
            .any(|sequence| {
                sequence
                    .trim_start_matches(|character: char| {
                        character.is_ascii_digit() || character == ';'
                    })
                    .starts_with('m')
            })
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
    assert!(!PromptColors::is_styled(&session), "{session:?}");
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
    assert!(!PromptColors::is_styled(&session), "{session:?}");
    assert!(session.contains("> "), "{session:?}");
    assert!(
        session.contains(&PromptColors::plain_exit_note()),
        "{session:?}"
    );
}

struct ValueColors;

impl ValueColors {
    const TAG: &str = "(defstruct tag :name string :size i64)\n";
    const TAG_FORM: &str = "(->tag :name (str \"an\" \"a\") :size (+ 1 1))";
    const TAG_VALUE: &str = "(->tag :name \"ana\" :size 2)";
    const PROGRAM_COLORS: &str = "\u{1b}[35mviolet\u{1b}[0m";

    fn painted_value() -> String {
        let keyword = anstyle::AnsiColor::Cyan.on_default();
        format!(
            "(->tag {} {} {} {})",
            Self::styled(keyword, ":name"),
            Self::styled(anstyle::AnsiColor::Green.on_default(), "\"ana\""),
            Self::styled(keyword, ":size"),
            Self::styled(anstyle::AnsiColor::Yellow.on_default(), "2"),
        )
    }

    fn styled(style: anstyle::Style, text: &str) -> String {
        format!("{style}{text}{style:#}")
    }

    fn assert_painted(output: &str) {
        let painted = Self::painted_value();
        assert!(output.contains(&painted), "{painted:?} in {output:?}");
    }

    fn assert_plain(output: &str) {
        assert!(output.contains(Self::TAG_VALUE), "{output:?}");
    }

    fn typed_value() -> String {
        format!("{}\r", Self::TAG_FORM)
    }

    fn session(name: &str, environment: &[(&str, &str)], arguments: &[&str]) -> String {
        let environment: Vec<(&str, &str)> = [("TERM", "xterm")]
            .into_iter()
            .chain(environment.iter().copied())
            .collect();
        let typed_tag = Self::TAG.replace('\n', "\r");
        let typed_value = Self::typed_value();
        Sandbox::new(name)
            .compiler_on_a_terminal_reading(
                &[
                    Keys::AtPrompt(&typed_tag),
                    Keys::AtPrompt(&typed_value),
                    Keys::AtPrompt(Keys::END_OF_INPUT),
                ],
                &environment,
                arguments,
            )
            .assert_exit_code(0)
            .stdout()
    }

    fn eval_on_a_terminal(name: &str, environment: &[(&str, &str)], forms: &str) -> Outcome {
        Self::eval_on_a_terminal_then(name, environment, forms, &[])
    }

    fn eval_on_a_terminal_then(
        name: &str,
        environment: &[(&str, &str)],
        forms: &str,
        shell_words: &[&str],
    ) -> Outcome {
        let environment: Vec<(&str, &str)> = [("TERM", "xterm")]
            .into_iter()
            .chain(environment.iter().copied())
            .collect();
        let quoted = format!("'{forms}'");
        let arguments: Vec<&str> = ["eval", "tags.rslisp", &quoted]
            .into_iter()
            .chain(shell_words.iter().copied())
            .collect();
        Sandbox::new(name)
            .with_file("tags.rslisp", Self::TAG)
            .compiler_on_a_terminal(&environment, &arguments)
    }
}

#[test]
fn color_repl_values_by_token_kind() {
    let session = ValueColors::session("color_repl_values", &[], &["repl"]);
    ValueColors::assert_painted(&session);
}

#[test]
fn color_repl_passes_the_program_escape_codes_unchanged() {
    let sandbox = Sandbox::new("color_repl_program_codes");
    let session = sandbox
        .compiler_on_a_terminal_reading(
            &[
                Keys::AtPrompt("(println \"\\u{1b}[35mviolet\\u{1b}[0m\")\r"),
                Keys::AtPrompt(Keys::END_OF_INPUT),
            ],
            &[("TERM", "xterm")],
            &["repl"],
        )
        .assert_exit_code(0)
        .stdout();
    assert!(session.contains(ValueColors::PROGRAM_COLORS), "{session:?}");
}

#[test]
fn color_repl_shows_the_program_prompt_before_it_reads() {
    let sandbox = Sandbox::new("color_repl_program_prompt");
    let session = sandbox
        .compiler_on_a_terminal_reading(
            &[
                Keys::AtPrompt("(do (println (str \"na\" \"me?\")) (read-line))\r"),
                Keys::AfterOutput("name?", "ana\r"),
                Keys::AtPrompt(Keys::END_OF_INPUT),
            ],
            &[("TERM", "xterm")],
            &["repl"],
        )
        .assert_exit_code(0)
        .stdout();
    let painted = ValueColors::styled(anstyle::AnsiColor::Green.on_default(), "\"ana\"");
    assert!(session.contains(&painted), "{session:?}");
}

#[test]
fn color_stays_off_repl_values_when_no_color_is_set() {
    let session = ValueColors::session(
        "color_repl_values_no_color",
        &[("NO_COLOR", "1")],
        &["repl"],
    );
    ValueColors::assert_plain(&session);
}

#[test]
fn color_never_keeps_repl_values_plain() {
    let session = ValueColors::session(
        "color_repl_values_never",
        &[],
        &["--color", "never", "repl"],
    );
    ValueColors::assert_plain(&session);
}

#[test]
fn color_stays_off_repl_values_in_a_pipe() {
    let input = format!("{}{}\n", ValueColors::TAG, ValueColors::TAG_FORM);
    let repl = Sandbox::new("color_repl_values_pipe").compiler_reading(input, &["repl"]);
    let output = repl.assert_exit_code(0).stdout();
    assert!(!output.contains(Outcome::ESCAPE), "{output:?}");
    ValueColors::assert_plain(&output);
}

#[test]
fn color_eval_values_on_a_terminal() {
    let eval = ValueColors::eval_on_a_terminal("color_eval_values", &[], ValueColors::TAG_FORM);
    ValueColors::assert_painted(&eval.assert_exit_code(0).stdout());
}

#[test]
fn color_stays_off_eval_values_when_no_color_is_set() {
    let eval = ValueColors::eval_on_a_terminal(
        "color_eval_values_no_color",
        &[("NO_COLOR", "1")],
        ValueColors::TAG_FORM,
    );
    ValueColors::assert_plain(&eval.assert_exit_code(0).stdout());
}

#[test]
fn color_eval_keeps_a_piped_value_plain_and_colors_the_diagnostics() {
    let forms = format!("(iter (vec 1 2)) {}", ValueColors::TAG_FORM);
    let eval =
        ValueColors::eval_on_a_terminal_then("color_eval_piped_value", &[], &forms, &["|", "cat"]);
    let output = eval.assert_exit_code(0).stdout();
    ValueColors::assert_plain(&output);
    assert!(output.contains("note"), "{output:?}");
    assert!(eval.stdout_is_colored(), "{output:?}");
}

struct DocColors;

impl DocColors {
    const SCALE_PROGRAM: &str = "(defn scale (:size i64 :factor i64 (defaults :factor 2))\n  \"Scales :size by \\\"factor\\\" 2.\"\n  (* size factor))\n";
    const SCALE_BODY: &str = "\n\nScales :size by \"factor\" 2.\n";
    const SCALE_HEADER: &str = "(defn scale (:size i64 :factor i64 (defaults :factor 2)))";

    fn painted_header() -> String {
        let keyword = |text| ValueColors::styled(anstyle::AnsiColor::Cyan.on_default(), text);
        let number = ValueColors::styled(anstyle::AnsiColor::Yellow.on_default(), "2");
        format!(
            "(defn scale ({} i64 {} i64 (defaults {} {number})))",
            keyword(":size"),
            keyword(":factor"),
            keyword(":factor"),
        )
    }

    fn page_on_a_terminal(name: &str, environment: &[(&str, &str)], flags: &[&str]) -> String {
        let environment: Vec<(&str, &str)> = [("TERM", "xterm")]
            .into_iter()
            .chain(environment.iter().copied())
            .collect();
        let arguments: Vec<&str> = ["doc"]
            .into_iter()
            .chain(flags.iter().copied())
            .chain(["--file", "scale.rslisp", "scale"])
            .collect();
        Sandbox::new(name)
            .with_file("scale.rslisp", Self::SCALE_PROGRAM)
            .compiler_on_a_terminal(&environment, &arguments)
            .assert_exit_code(0)
            .stdout()
            .replace("\r\n", "\n")
    }
}

#[test]
fn color_doc_header_on_a_terminal_and_leave_the_docstring_plain() {
    let page = DocColors::page_on_a_terminal("color_doc_header", &[], &[]);
    let expected = format!("{}{}", DocColors::painted_header(), DocColors::SCALE_BODY);
    assert!(page.ends_with(&expected), "{page:?}");
}

#[test]
fn color_doc_header_below_the_type_line() {
    let page = DocColors::page_on_a_terminal("color_doc_typed_header", &[], &["--types"]);
    let expected = format!("{}{}", DocColors::painted_header(), DocColors::SCALE_BODY);
    assert!(page.ends_with(&expected), "{page:?}");
}

#[test]
fn color_stays_off_the_doc_header_when_no_color_is_set() {
    let page = DocColors::page_on_a_terminal("color_doc_no_color", &[("NO_COLOR", "1")], &[]);
    let expected = format!("{}{}", DocColors::SCALE_HEADER, DocColors::SCALE_BODY);
    assert!(page.ends_with(&expected), "{page:?}");
}

struct RedefinitionSession;

impl RedefinitionSession {
    fn transcript(name: &str, input: &str) -> String {
        let sandbox = Sandbox::new(name).with_file("shapes.rslisp", SHAPES_OF_TWO);
        sandbox.compiler_reading(input, &["repl"]).transcript()
    }

    fn after_a_module_change(rest: &str) -> String {
        format!(
            "{WRITE_FILE}(use shapes circle square)\n(defn area () (square 2))\n{SHAPES_WITHOUT_SQUARE}{rest}"
        )
    }
}

#[test]
fn repl_sets_aside_an_input_that_a_redefinition_breaks() {
    insta::assert_snapshot!(RedefinitionSession::transcript(
        "repl_redefined_struct",
        PROMPT_REDEFINED_STRUCT
    ));
}

#[test]
fn repl_rejects_a_failed_redefinition_and_keeps_the_old_definition() {
    insta::assert_snapshot!(RedefinitionSession::transcript(
        "repl_failed_redefined_struct",
        PROMPT_FAILED_REDEFINED_STRUCT
    ));
}

#[test]
fn repl_sets_aside_the_inputs_that_a_set_aside_input_breaks() {
    insta::assert_snapshot!(RedefinitionSession::transcript(
        "repl_cascade",
        PROMPT_CASCADE
    ));
}

#[test]
fn repl_rejects_a_call_to_a_broken_macro() {
    insta::assert_snapshot!(RedefinitionSession::transcript(
        "repl_broken_macro_called",
        PROMPT_BROKEN_MACRO_CALLED
    ));
}

#[test]
fn repl_rejects_a_broken_macro_that_an_earlier_input_calls() {
    insta::assert_snapshot!(RedefinitionSession::transcript(
        "repl_broken_macro_redefined",
        PROMPT_BROKEN_MACRO_REDEFINED
    ));
}

#[test]
fn repl_notes_a_name_that_an_input_stops_binding() {
    insta::assert_snapshot!(RedefinitionSession::transcript(
        "repl_stops_binding",
        PROMPT_STOPS_BINDING
    ));
}

#[test]
fn repl_sets_aside_the_later_of_two_inputs_that_bind_one_name() {
    insta::assert_snapshot!(RedefinitionSession::transcript(
        "repl_two_binders",
        PROMPT_TWO_BINDERS
    ));
}

#[test]
fn repl_notes_what_a_rejected_input_would_set_aside() {
    insta::assert_snapshot!(RedefinitionSession::transcript(
        "repl_would_set_aside",
        PROMPT_WOULD_SET_ASIDE
    ));
}

#[test]
fn repl_sets_aside_the_inputs_of_a_module_changed_on_disk() {
    let input = RedefinitionSession::after_a_module_change("(defn z () 1)\n(area)\n");
    insta::assert_snapshot!(RedefinitionSession::transcript(
        "repl_module_changed",
        &input
    ));
}

#[test]
fn repl_helps_an_expression_after_a_module_change() {
    let input = RedefinitionSession::after_a_module_change("(circle 2)\n");
    insta::assert_snapshot!(RedefinitionSession::transcript(
        "repl_expression_after_module_change",
        &input
    ));
}

#[test]
fn repl_forgets_the_record_of_a_name_entered_again() {
    let sandbox = Sandbox::new("repl_re_entered_type").with_file("geo.rslisp", GEO_WITHOUT_POINT);
    let repl = sandbox.compiler_reading(PROMPT_RE_ENTERED_TYPE, &["repl"]);
    insta::assert_snapshot!(repl.transcript());
}

#[test]
fn repl_gives_an_expression_calling_a_broken_macro_no_module_help() {
    insta::assert_snapshot!(RedefinitionSession::transcript(
        "repl_expression_broken_macro",
        PROMPT_EXPRESSION_CALLING_A_BROKEN_MACRO
    ));
}

#[test]
fn repl_warns_once_for_an_input_with_two_errors() {
    insta::assert_snapshot!(RedefinitionSession::transcript(
        "repl_two_errors_in_one_input",
        PROMPT_TWO_ERRORS_IN_ONE_INPUT
    ));
}

#[test]
fn repl_keeps_the_suggestion_of_a_module_typo_before_the_help_to_enter_again() {
    let input = format!("{WRITE_FILE}(use shapes circle)\n{SHAPES_WITH_A_TYPO}(defn z () 1)\n");
    insta::assert_snapshot!(RedefinitionSession::transcript("repl_module_typo", &input));
}

#[test]
fn repl_helps_an_expression_whose_error_lies_in_a_changed_module() {
    let input = format!("{WRITE_FILE}(use shapes circle)\n{SHAPES_WITH_A_TYPO}(circle 2)\n");
    insta::assert_snapshot!(RedefinitionSession::transcript(
        "repl_expression_module_typo",
        &input
    ));
}

struct EditedSession {
    sandbox: Sandbox,
}

impl EditedSession {
    const INPUT_LINE: &str = "(f :x 1 \"s\") ; c\r";

    fn new(name: &str) -> Self {
        Self {
            sandbox: Sandbox::new(name),
        }
    }

    fn typing(&self, typed: &[Keys], environment: &[(&str, &str)]) -> Outcome {
        let environment: Vec<(&str, &str)> = [("TERM", "xterm")]
            .into_iter()
            .chain(environment.iter().copied())
            .collect();
        self.sandbox
            .compiler_on_a_terminal_reading(typed, &environment, &["repl"])
    }

    fn session(&self, typed: &[Keys], environment: &[(&str, &str)]) -> String {
        self.typing(typed, environment).assert_exit_code(0).stdout()
    }

    fn with_tool(self, name: &str, script: &str) -> Self {
        Self {
            sandbox: self
                .sandbox
                .with_executable(&format!("tools/{name}"), script),
        }
    }

    fn path_with_tools(&self) -> String {
        format!(
            "{}:{}",
            self.sandbox.directory.join("tools").display(),
            std::env::var("PATH").expect("the tests run with a PATH")
        )
    }

    fn history(&self) -> String {
        self.sandbox
            .written_file(&Self::history_path(Sandbox::STATE_DIRECTORY))
    }

    fn history_path(state: &str) -> String {
        format!("{state}/{}/history", Language::NAME)
    }

    fn styled(style: anstyle::Style, text: &str) -> String {
        format!("{style}{text}{style:#}")
    }

    fn printed_number(number: &str) -> String {
        let painted = Self::styled(anstyle::AnsiColor::Yellow.on_default(), number);
        format!("\r\n{painted}\r\n")
    }

    fn history_notes(session: &str) -> usize {
        session.matches("history is off").count()
    }
}

#[test]
fn repl_edits_a_line_on_a_terminal() {
    let edited = EditedSession::new("repl_edited_line");
    let typed = format!("(+ 2){} 5\r", Keys::LEFT);
    let session = edited.session(
        &[Keys::AtPrompt(&typed), Keys::AtPrompt(Keys::END_OF_INPUT)],
        &[("NO_COLOR", "1")],
    );
    assert!(session.contains("(+ 2 5)"), "{session:?}");
    assert!(session.contains("\r\n7\r\n"), "{session:?}");
}

#[test]
fn repl_indents_a_typed_line_inside_an_unclosed_list() {
    let edited = EditedSession::new("repl_typed_continuation");
    let session = edited.session(
        &[
            Keys::AtPrompt("(defn twice (x)\r(* 2 x))\r"),
            Keys::AtPrompt("(twice 4)\r"),
            Keys::AtPromptAfter("\r\n8\r\n", Keys::END_OF_INPUT),
        ],
        &[("NO_COLOR", "1")],
    );
    let screen = RenderedScreen::of(&session);
    assert!(
        screen.contains("> (defn twice (x)\n  (* 2 x))\n"),
        "{screen}"
    );
}

#[test]
fn repl_adds_no_spaces_inside_an_unclosed_string() {
    let edited = EditedSession::new("repl_string_continuation");
    let session = edited.session(
        &[
            Keys::AtPrompt("(println \"a\rb\")\r"),
            Keys::AtPromptAfter("\r\na\r\nb\r\n", Keys::END_OF_INPUT),
        ],
        &[("NO_COLOR", "1")],
    );
    let screen = RenderedScreen::of(&session);
    assert!(screen.contains("> (println \"a\nb\")\na\nb\n"), "{screen}");
}

#[test]
fn repl_reads_plain_lines_on_an_unsupported_terminal() {
    let edited = EditedSession::new("repl_dumb_terminal");
    let session = edited.session(
        &[
            Keys::AfterOutput("> ", "(+ 1 2)\n"),
            Keys::AfterOutput("\r\n3\r\n> ", "(+ 1\n"),
            Keys::AfterOutput("  ", Keys::END_OF_INPUT),
        ],
        &[("TERM", "dumb")],
    );
    assert!(session.contains("error[L0101]"), "{session:?}");
    assert!(
        !session.contains(TerminalScreen::PROMPT_READY),
        "{session:?}"
    );
}

#[test]
fn repl_shows_a_multi_line_paste_once() {
    let edited = EditedSession::new("repl_paste");
    let paste = format!(
        "{}(defn twice (x)\n  (* 2 x))\n(twice 4)\n{}\r",
        Keys::PASTE_START,
        Keys::PASTE_END
    );
    let session = edited.session(
        &[Keys::AtPrompt(&paste), Keys::AtPrompt(Keys::END_OF_INPUT)],
        &[("NO_COLOR", "1")],
    );
    assert!(
        session.contains("> (defn twice (x)\r\n  (* 2 x))\r\n(twice 4)\r\n"),
        "{session:?}"
    );
    assert!(session.contains("\r\n8\r\n"), "{session:?}");
    assert!(!session.contains("  > "), "{session:?}");
}

#[test]
fn repl_keeps_lines_typed_ahead_of_the_prompt() {
    let edited = EditedSession::new("repl_typed_ahead");
    let session = edited.session(
        &[
            Keys::AtPrompt("(+ 1 2)\r(+ 3\r 4)\r"),
            Keys::AtPromptAfter("\r\n7\r\n", Keys::END_OF_INPUT),
        ],
        &[("NO_COLOR", "1")],
    );
    assert!(session.contains("\r\n3\r\n"), "{session:?}");
    assert!(session.contains(" 4)\u{1b}[?2004l\r\n7\r\n"), "{session:?}");
}

#[test]
fn repl_colors_the_input_as_it_is_typed() {
    let edited = EditedSession::new("repl_input_colors");
    let session = edited
        .typing(
            &[
                Keys::AtPrompt(EditedSession::INPUT_LINE),
                Keys::AtPrompt(Keys::END_OF_INPUT),
            ],
            &[],
        )
        .stdout();
    for (color, text) in [
        (anstyle::AnsiColor::Cyan.on_default(), ":x"),
        (anstyle::AnsiColor::Yellow.on_default(), "1"),
        (anstyle::AnsiColor::Green.on_default(), "\"s\""),
        (anstyle::Style::new().dimmed(), "; c"),
    ] {
        let styled = EditedSession::styled(color, text);
        assert!(session.contains(&styled), "{styled:?} in {session:?}");
    }
}

#[test]
fn repl_leaves_the_input_plain_when_color_is_off() {
    let edited = EditedSession::new("repl_input_plain");
    let session = edited
        .typing(
            &[
                Keys::AtPrompt(EditedSession::INPUT_LINE),
                Keys::AtPrompt(Keys::END_OF_INPUT),
            ],
            &[("NO_COLOR", "1")],
        )
        .stdout();
    assert!(session.contains("(f :x 1 \"s\") ; c"), "{session:?}");
    assert!(!PromptColors::is_styled(&session), "{session:?}");
}

#[test]
fn repl_quits_at_an_empty_prompt_on_end_of_input() {
    let edited = EditedSession::new("repl_end_of_input");
    let quit = edited.typing(&[Keys::AtPrompt(Keys::END_OF_INPUT)], &[]);
    assert_eq!(quit.assert_exit_code(0).stderr(), "");
}

#[test]
fn repl_drops_the_typed_input_on_interrupt() {
    let edited = EditedSession::new("repl_interrupt_typing");
    let interrupted = format!("(+ 1{}", Keys::INTERRUPT);
    let session = edited.session(
        &[
            Keys::AtPrompt(&interrupted),
            Keys::AtPrompt("(+ 2 3)\r"),
            Keys::AtPrompt(Keys::END_OF_INPUT),
        ],
        &[],
    );
    assert!(
        session.contains(&EditedSession::printed_number("5")),
        "{session:?}"
    );
    assert_eq!(edited.history(), "#V2\n(+ 2 3)\n");
}

#[test]
fn repl_reports_unreadable_input_on_a_terminal() {
    let edited = EditedSession::new("repl_unreadable_terminal");
    let session = edited
        .typing(&[Keys::BytesAtPrompt(b"\xff\r")], &[("NO_COLOR", "1")])
        .assert_exit_code(2)
        .stdout();
    assert!(
        session.contains("error: cannot read standard input: invalid data"),
        "{session:?}"
    );
}

#[test]
fn repl_keeps_history_under_the_state_home() {
    let edited = EditedSession::new("repl_history");
    edited.session(
        &[
            Keys::AtPrompt("(+ 1 2)\r"),
            Keys::AtPrompt(Keys::END_OF_INPUT),
        ],
        &[],
    );
    let recalled = edited.session(
        &[
            Keys::AtPrompt("\u{1b}[A\r"),
            Keys::AtPrompt(Keys::END_OF_INPUT),
        ],
        &[],
    );
    assert!(
        recalled.contains(&EditedSession::printed_number("3")),
        "{recalled:?}"
    );
    assert_eq!(edited.history(), "#V2\n(+ 1 2)\n");
}

#[test]
fn repl_keeps_history_under_the_home_directory() {
    let edited = EditedSession::new("repl_history_home");
    let home = edited.sandbox.directory.join("home");
    let home = home.to_str().expect("the sandbox path is UTF-8");
    edited.session(
        &[
            Keys::AtPrompt("(+ 1 2)\r"),
            Keys::AtPrompt(Keys::END_OF_INPUT),
        ],
        &[("XDG_STATE_HOME", ""), ("HOME", home)],
    );
    let path = EditedSession::history_path("home/.local/state");
    assert_eq!(edited.sandbox.written_file(&path), "#V2\n(+ 1 2)\n");
}

#[test]
fn repl_works_without_history_when_no_state_directory_is_known() {
    let edited = EditedSession::new("repl_history_unknown");
    let session = edited.session(
        &[
            Keys::AtPrompt("(+ 1 2)\r"),
            Keys::AtPrompt(Keys::END_OF_INPUT),
        ],
        &[("XDG_STATE_HOME", ""), ("HOME", "")],
    );
    assert!(
        session.contains("history is off: no state directory"),
        "{session:?}"
    );
    assert!(
        session.contains(&EditedSession::printed_number("3")),
        "{session:?}"
    );
}

#[test]
fn repl_works_without_history_when_the_state_directory_cannot_be_made() {
    let edited = EditedSession::new("repl_history_blocked");
    fs::write(edited.sandbox.directory.join("blocked"), "").expect("the sandbox accepts files");
    let blocked = edited.sandbox.directory.join("blocked");
    let blocked = blocked.to_str().expect("the sandbox path is UTF-8");
    let session = edited.session(
        &[
            Keys::AtPrompt("(+ 1 2)\r"),
            Keys::AtPrompt(Keys::END_OF_INPUT),
        ],
        &[("XDG_STATE_HOME", blocked)],
    );
    assert!(
        session.contains("history is off: cannot write"),
        "{session:?}"
    );
    assert_eq!(EditedSession::history_notes(&session), 1, "{session:?}");
}

#[test]
fn repl_works_without_history_when_the_history_cannot_be_read() {
    let edited = EditedSession::new("repl_history_unreadable");
    let path = EditedSession::history_path(Sandbox::STATE_DIRECTORY);
    fs::create_dir_all(edited.sandbox.directory.join(&path))
        .expect("the sandbox accepts directories");
    let session = edited.session(
        &[
            Keys::AtPrompt("(+ 1 2)\r"),
            Keys::AtPrompt(Keys::END_OF_INPUT),
        ],
        &[],
    );
    assert!(
        session.contains("history is off: cannot read"),
        "{session:?}"
    );
    assert!(
        session.contains(&EditedSession::printed_number("3")),
        "{session:?}"
    );
}

#[test]
fn repl_says_once_that_history_cannot_be_written() {
    let edited = EditedSession::new("repl_history_read_only");
    let path = EditedSession::history_path(Sandbox::STATE_DIRECTORY);
    let file = edited.sandbox.directory.join(&path);
    fs::create_dir_all(file.parent().expect("the history has a directory"))
        .expect("the sandbox accepts directories");
    fs::write(&file, "#V2\n").expect("the sandbox accepts files");
    fs::set_permissions(&file, fs::Permissions::from_mode(Sandbox::READ_ONLY))
        .expect("the sandbox sets permissions");
    let session = edited.session(
        &[
            Keys::AtPrompt("(+ 1 2)\r"),
            Keys::AtPrompt("(+ 3 4)\r"),
            Keys::AtPrompt(Keys::END_OF_INPUT),
        ],
        &[],
    );
    assert!(
        session.contains("history is off: cannot write"),
        "{session:?}"
    );
    assert_eq!(EditedSession::history_notes(&session), 1, "{session:?}");
    assert!(
        session.contains(&EditedSession::printed_number("7")),
        "{session:?}"
    );
}

#[test]
fn build_stops_quietly_when_rustc_is_interrupted() {
    let sandbox = Sandbox::new("build_rustc_interrupted")
        .with_file("greeting.rslisp", GREETING)
        .with_executable("tools/rustc", INTERRUPTED_RUSTC)
        .with_executable("tools/rustfmt", PASSTHROUGH_RUSTFMT);
    let build = sandbox.compiler_with_path("tools", &["build", "greeting.rslisp"]);
    assert_eq!(build.assert_exit_code(130).stderr(), "");
}

#[test]
fn repl_interrupts_a_running_program_and_keeps_the_definitions() {
    let edited = EditedSession::new("repl_interrupt_program");
    let session = edited.session(
        &[
            Keys::AtPrompt("(defn spin (i) (if (< i 0) i (recur (+ i 1))))\r"),
            Keys::AtPrompt("(do (println \"spinning\") (spin 0)) (+ 40 2)\r"),
            Keys::AfterOutput("spinning\r\n", Keys::INTERRUPT),
            Keys::AtPromptAfter("interrupted\r\n", "(spin -2)\r"),
            Keys::AtPromptAfter("\r\n-2\r\n", Keys::END_OF_INPUT),
        ],
        &[("NO_COLOR", "1")],
    );
    assert!(!session.contains("exited with code"), "{session:?}");
    assert!(!session.contains("\r\n42\r\n"), "{session:?}");
    assert_eq!(session.matches("interrupted").count(), 1, "{session:?}");
}

#[test]
fn repl_interrupts_rustc_without_an_error() {
    let edited = EditedSession::new("repl_interrupt_rustc").with_tool("rustc", WAITING_RUSTC);
    let session = edited.session(
        &[
            Keys::AtPrompt("(+ 1 2)\r"),
            Keys::AfterOutput("compiling\r\n", Keys::INTERRUPT),
            Keys::AtPromptAfter("interrupted\r\n", Keys::END_OF_INPUT),
        ],
        &[("NO_COLOR", "1"), ("PATH", &edited.path_with_tools())],
    );
    assert!(!session.contains("error"), "{session:?}");
}

#[test]
fn repl_interrupts_a_rustc_that_catches_the_signal() {
    let edited = EditedSession::new("repl_interrupt_catching_rustc")
        .with_tool("rustc", SIGNAL_CATCHING_RUSTC);
    let session = edited.session(
        &[
            Keys::AtPrompt("(+ 1 2)\r"),
            Keys::AfterOutput("compiling\r\n", Keys::INTERRUPT),
            Keys::AtPromptAfter("interrupted\r\n", Keys::END_OF_INPUT),
        ],
        &[("NO_COLOR", "1"), ("PATH", &edited.path_with_tools())],
    );
    assert!(!session.contains("error"), "{session:?}");
}

#[test]
fn repl_reports_a_rustc_failure_on_a_terminal() {
    let edited = EditedSession::new("repl_rustc_failure_terminal")
        .with_tool("rustc", CRASHING_RUSTC)
        .with_tool("rustfmt", FAILING_RUSTFMT);
    let session = edited.session(
        &[
            Keys::AtPrompt("(+ 1 2)\r"),
            Keys::AtPromptAfter("rustc crashed", Keys::END_OF_INPUT),
        ],
        &[("NO_COLOR", "1"), ("PATH", &edited.path_with_tools())],
    );
    assert!(session.contains("note: rustfmt failed"), "{session:?}");
    assert!(!session.contains("interrupted"), "{session:?}");
}

#[test]
fn repl_interrupts_a_rustfmt_that_catches_the_signal() {
    let edited = EditedSession::new("repl_interrupt_catching_rustfmt")
        .with_tool("rustfmt", SIGNAL_CATCHING_RUSTFMT);
    let session = edited.session(
        &[
            Keys::AtPrompt("(+ 40 2)\r"),
            Keys::AfterOutput("formatting\r\n", Keys::INTERRUPT),
            Keys::AtPromptAfter("interrupted\r\n", Keys::END_OF_INPUT),
        ],
        &[("NO_COLOR", "1"), ("PATH", &edited.path_with_tools())],
    );
    assert!(!session.contains("note"), "{session:?}");
    assert!(!session.contains("\r\n42\r\n"), "{session:?}");
}

#[test]
fn repl_interrupts_a_running_program_on_an_unsupported_terminal() {
    let edited = EditedSession::new("repl_interrupt_dumb_terminal");
    let session = edited.session(
        &[
            Keys::AfterOutput("> ", "(defn spin (i) (if (< i 0) i (recur (+ i 1))))\n"),
            Keys::AfterOutput("> ", "(do (println \"spinning\") (spin 0))\n"),
            Keys::AfterOutput("spinning\r\n", Keys::INTERRUPT),
            Keys::AfterOutput("interrupted\r\n> ", "(spin -2)\n"),
            Keys::AfterOutput("\r\n-2\r\n> ", Keys::END_OF_INPUT),
        ],
        &[("TERM", "dumb")],
    );
    assert!(!session.contains("exited with code"), "{session:?}");
}

#[test]
fn emit_stops_quietly_when_rustfmt_is_interrupted() {
    let sandbox = Sandbox::new("emit_rustfmt_interrupted")
        .with_file("greeting.rslisp", GREETING)
        .with_executable("tools/rustfmt", INTERRUPTED_RUSTFMT);
    let emit = sandbox.compiler_with_path("tools", &["emit", "greeting.rslisp"]);
    assert_eq!(emit.assert_exit_code(130).stderr(), "");
}

#[test]
fn emit_stops_quietly_when_rustfmt_is_interrupted_before_it_reads_its_input() {
    let sandbox = Sandbox::new("emit_rustfmt_interrupted_before_reading")
        .with_file("greeting.rslisp", &Sandbox::pipe_overflowing_greeting())
        .with_executable("tools/rustfmt", INTERRUPTED_RUSTFMT);
    let emit = sandbox.compiler_with_path("tools", &["emit", "greeting.rslisp"]);
    assert_eq!(emit.assert_exit_code(130).stderr(), "");
}

#[test]
fn emit_notes_a_rustfmt_that_fails_before_it_reads_its_input() {
    let sandbox = Sandbox::new("emit_rustfmt_failed_before_reading")
        .with_file("greeting.rslisp", &Sandbox::pipe_overflowing_greeting())
        .with_executable("tools/rustfmt", FAILING_RUSTFMT);
    let emit = sandbox.compiler_with_path("tools", &["emit", "greeting.rslisp"]);
    assert!(
        emit.assert_exit_code(0).stderr().contains("Broken pipe"),
        "{}",
        emit.stderr()
    );
}

#[test]
fn repl_interrupts_rustfmt_and_skips_the_input() {
    let edited = EditedSession::new("repl_interrupt_rustfmt").with_tool("rustfmt", WAITING_RUSTFMT);
    let session = edited.session(
        &[
            Keys::AtPrompt("(+ 40 2)\r"),
            Keys::AfterOutput("formatting\r\n", Keys::INTERRUPT),
            Keys::AtPromptAfter("interrupted\r\n", Keys::END_OF_INPUT),
        ],
        &[("NO_COLOR", "1"), ("PATH", &edited.path_with_tools())],
    );
    assert!(!session.contains("note"), "{session:?}");
    assert!(!session.contains("\r\n42\r\n"), "{session:?}");
}

#[test]
fn repl_skips_rustc_after_an_interrupt_in_the_front_end() {
    let edited =
        EditedSession::new("repl_interrupt_front_end").with_tool("rustfmt", UNINTERRUPTED_RUSTFMT);
    let session = edited.session(
        &[
            Keys::AtPrompt("(+ 40 2)\r"),
            Keys::AfterOutput("formatting\r\n", Keys::INTERRUPT),
            Keys::AtPromptAfter("interrupted\r\n", "(+ 1 2)\r"),
            Keys::AtPromptAfter("\r\n3\r\n", Keys::END_OF_INPUT),
        ],
        &[("NO_COLOR", "1"), ("PATH", &edited.path_with_tools())],
    );
    assert!(!session.contains("\r\n42\r\n"), "{session:?}");
}

#[test]
fn repl_on_piped_input_stops_at_an_interrupt() {
    let sandbox = Sandbox::new("repl_piped_interrupt");
    let mut child = sandbox
        .command(Path::new(Sandbox::COMPILER), &["repl"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the compiler starts");
    let mut input = child.stdin.take().expect("stdin is piped");
    std::io::Write::write_all(&mut input, b"(+ 1 2)\n").expect("the compiler reads its input");
    let mut first_value = String::new();
    BufReader::new(child.stdout.take().expect("stdout is piped"))
        .read_line(&mut first_value)
        .expect("the compiler prints the value");
    assert_eq!(first_value, "3\n");
    let interrupt = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(interrupt.success());
    let deadline = Instant::now() + Duration::from_secs(30);
    while child
        .try_wait()
        .expect("the compiler is waitable")
        .is_none()
    {
        if Instant::now() > deadline {
            child.kill().ok();
            panic!("the piped prompt kept running after an interrupt");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(input);
    let status = child.wait().expect("the compiler finishes");
    assert_eq!(status.signal(), Some(2));
}
