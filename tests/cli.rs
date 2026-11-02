use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use rslisp::build::EmittedFormatting;
use serde_json::{Map, Value, json};

const GREETING: &str = "(defn main ()\n  (println \"hello from the sandbox\"))\n";
const PRINTS_A: &str = "(defn main () (println \"A\"))\n";
const PRINTS_B: &str = "(defn main () (println \"B\"))\n";
const MARKED_PROGRAM: &str = "\u{feff}(defn main () (println 1))\n";
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
const COUNT_INPUT_LINES: &str = "(defn main ()\n  (println (count (line-seq stdin))))\n";
const STRUCTS: &str = "(defstruct person :name string :age i64)\n(defstruct ticket :id u32)\n(derive ticket debug)\n\n(defn main () (do))\n";
const BOUND_ENTRY: &str = "(use other/person person)\n\n(rust-trait same \"std::ops::Add\" i64 (with i64 person))\n\n(type matches (where ((a (same person))) (fn (a) bool)))\n(defn matches (x) true)\n\n(defn main () (println (matches 1)))\n";

const TRAIT_SHARED_VECTOR: &str = "(struct odometer (offset i64))\n\n(trait mileage\n  (first-reading (fn ((ref self) (ref (vec i64))) i64)))\n\n(impl mileage odometer\n  (first-reading (o readings) (+ (field o offset) (at readings 0))))\n\n(defn main ()\n  (println (first-reading (new odometer (offset 10)) (vec 1200))))\n";
const GEOMETRY: &str = "(export area)\n\n(defn area (width height) \"Multiplies a width by a height.\" (* width height))\n";
const GEOMETRY_ENTRY: &str = "(use geometry area)\n\n(defn main () (println (area 3 4)))\n";
const UNDOCUMENTED_ENTRY: &str = "(defn twice (x) (* 2 x))\n\n(defn main () (println (twice 2)))\n";
const SHADOWING_DOC: &str = "(defrule doc ((term) 0))\n\n(defn main () (do))\n";
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
const PAGE_PROGRAM: &str = "(defn twice (x) \"Doubles a number.\" (* 2 x))\n(def page (doc twice))\n(defn main () (println page))\n";
const SHADOWING_SUM: &str =
    "(defn sum (left right) (+ left right))\n(defn main () (println (sum 1 2)))\n";
const NUMBERS: &str = "(export twice)\n\n(defn twice (x) \"Doubles a number.\" (* 2 x))\n";
const NUMBERS_ENTRY: &str = "(use lib/numbers twice)\n\n(defn main () (println (twice 21)))\n";
const TWICE_TYPE: &str = "(type twice (fn (i64) i64))";
const TWICE_PAGE: &str = "(defn twice (x))\n\nDoubles a number.\n";
const POINT_TYPE: &str = "(type point (fn () point))";

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

    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.output.stdout).into_owned()
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
fn doc_rejects_a_shadowing_doc_that_returns_no_string() {
    let sandbox = Sandbox::new("doc_shadowed").with_file("main.rslisp", SHADOWING_DOC);
    let doc = sandbox.compiler(&["doc", "--file", "main.rslisp", "main"]);
    assert_eq!(
        doc.assert_exit_code(1).stderr(),
        "error: `doc` did not expand to a string\n"
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
