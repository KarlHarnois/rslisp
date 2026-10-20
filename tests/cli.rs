use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const GREETING: &str = "(defn main ()\n  (println \"hello from the sandbox\"))\n";
const UNBOUND_NAME: &str = "(defn main ()\n  (println greeting))\n";
const ECHO: &str = "(def first-argument (rust \"std::env::args().nth(1).unwrap_or_default()\" string))\n\n(defn main ()\n  (println first-argument))\n";
const EXIT_THREE: &str =
    "(def exit (rust \"std::process::exit({0})\" (fn (i32) unit)))\n\n(defn main ()\n  (exit 3))\n";
const RUSTC_SYNTAX_ERROR: &str = "(def doubled-sign (rust \"{0} + + 1\" (fn (i64) i64)))\n\n(defn main ()\n  (println (doubled-sign 1)))\n";

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

    fn with_file(self, name: &str, source: &str) -> Self {
        fs::write(self.directory.join(name), source).expect("the sandbox accepts files");
        self
    }

    fn compiler(&self, arguments: &[&str]) -> Outcome {
        self.execute(Path::new(Self::COMPILER), arguments)
    }

    fn built_program(&self, relative_path: &str) -> Outcome {
        self.execute(&self.directory.join(relative_path), &[])
    }

    fn execute(&self, program: &Path, arguments: &[&str]) -> Outcome {
        let output = Command::new(program)
            .args(arguments)
            .current_dir(&self.directory)
            .output()
            .unwrap_or_else(|error| panic!("cannot start `{}`: {error}", program.display()));
        Outcome { output }
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

    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.output.stdout).into_owned()
    }

    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.output.stderr).into_owned()
    }
}

#[test]
fn help_lists_every_command() {
    let sandbox = Sandbox::new("help");
    let help = sandbox.compiler(&["--help"]);
    insta::assert_snapshot!(help.assert_exit_code(0).stdout());
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
fn check_reports_errors_without_terminal_styling() {
    let sandbox = Sandbox::new("check_errors").with_file("unbound.rslisp", UNBOUND_NAME);
    let check = sandbox.compiler(&["check", "unbound.rslisp"]);
    insta::assert_snapshot!(check.assert_exit_code(1).stderr());
}

#[test]
fn expand_prints_kernel_forms() {
    let sandbox = Sandbox::new("expand").with_file("greeting.rslisp", GREETING);
    let expand = sandbox.compiler(&["expand", "greeting.rslisp"]);
    assert_eq!(
        expand.assert_exit_code(0).stdout(),
        "(def main (fn () (println \"hello from the sandbox\")))\n"
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
