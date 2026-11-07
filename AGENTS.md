# rslisp agent guide

This file and the reference files it names are the living reference for working in this repository. If a change makes a statement in any of them false, update the statement in the same commit.

## Terms

| Term | Meaning |
| --- | --- |
| Project owner | The person you work for in this repository |
| Kernel form | A form the compiler knows, listed by `KernelForm` in `expand/src/core/kernel.rs` |
| Prelude | The language's library under `prelude/`, written in the language and embedded in the compiler |
| Stage | One step of the compiler, such as reading, expansion or emission |
| Front end | Every stage before rustc |
| Fixture | A source file under `tests/fixtures/` whose output an insta snapshot records |
| Corpus entry | A top-level fixture under `tests/fixtures/corpus/`, sized to measure the compiler's performance |
| Working commands | The two commands at the top of the Commands section |

## Principles

**If no rule covers a case, choose what these principles favor.** If two principles conflict, the earlier principle wins.

### User experience first

- The end user's experience wins every trade-off against an easier implementation.
- User experience covers the emitted Rust, diagnostics, run output, compile speed and the predictability of the language's rules.
- NEVER accept worse output because it is easier to build, such as a confusing page or a doubled diagnostic.
- A language exception costs the user too: PREFER a uniform rule that gives the better output OVER a special case.

### Few rules, no exceptions

- The reader treats only `(`, `)`, `"`, `;` and whitespace specially.
- NEVER add a character that the reader treats specially without the project owner's approval.
- If a feature needs an exception to a rule of the syntax, redesign the feature or leave it out.

### Small compiler, big prelude

- Before you add a feature to the compiler, write the feature as a prelude macro or a prelude function.
- If the prelude can express a feature, NEVER add a kernel form for it.
- If you add a kernel form, record why the prelude cannot express it in `docs/language.md`.
- The compiler reads, checks, reports and emits. Every other feature belongs in the prelude.
- NEVER make the compiler depend on how the prelude implements a form. To learn what a macro does, expand it.
- NEVER restate in Rust a name or a fact that the prelude states, such as a trait's name. Read it from the prelude's bindings.

### Rust's performance and model

- NEVER emit a clone or an allocation that the source does not write, except the `Box` that a recursive enum position requires, because no program can build that type without it.
- Keep the emitted Rust readable as hand-written Rust.

### Let the code speak

- NEVER write prose that restates what the code shows, such as a file listing or the steps a function takes.
- If a name, a type, or a test can carry an intent, carry the intent there instead of in prose.
- If an automated check can enforce a rule, enforce the rule there instead of in prose.

## Ground rules

### Functions

- Keep each function small. Make each function do one thing at one level of abstraction.
- Place each caller above the functions it calls.
- NEVER write a flag argument.
- NEVER write a side effect that the function's name does not announce.
- Return a failure as a `Result`. Return absence as an `Option`.
- State each piece of knowledge once.

### Comments

**A comment is a failure to express intent in code.** A doc comment is a comment and follows the same rules.

For each comment:

1. Improve the code until the comment is unnecessary: rename, extract a function, introduce a type, or restructure.
2. If the comment does not fall under an exception below, delete the comment.
3. Otherwise, justify the comment to the project owner before you commit.

The exceptions are:

- a legal header
- non-obvious intent that structure cannot encode
- a warning of a real consequence
- a TODO that references a tracked issue

### Object-oriented Rust

- Put behavior in `impl` blocks. Inject dependencies through constructors.
- A type that carries behavior keeps its fields private.
- A type of the core IR, the typed IR, or another plain data structure MAY expose public fields.
- NEVER write a free function except `main` and `#[test]` functions: Rust requires those to be free.
- Make each test helper a method of a fixture struct. The stage fixtures share `Fixture` in `tests/support/mod.rs`.

### Names

- Name a type with a noun. Name a method that acts with a verb.
- NEVER put an abbreviation or a noise word in a name.
- Unless a number gives each name's position in a sequence, NEVER tell two names apart by a number.
- If a value's meaning is not obvious, make the value a named constant.
- In Rust code, take the language's name and file extension from `Language` in `expand/src/language.rs`: `rslisp` is a placeholder name.

### Stages

- A stage depends only on earlier stages. NEVER make a stage depend on a later one.
- A stage reports problems as `Diagnostic` values. NEVER print from a stage.
- Only `src/cli/` prints, except the directives that `build.rs` gives cargo.
- Only `src/cli/`, `src/build/`, `ModuleSources`, `build.rs`, which writes only to `OUT_DIR`, and `xtask/` touch the file system or run processes.
- The only `build.rs` builds the prelude state. NEVER add another build script or give this one another job.
- Write the wording of every diagnostic that a compiler stage reports in the stage's problems type, such as `ExpandProblems` or `TypeProblems`.
- A prelude `fail` writes its message, help and labels in the prelude.

### Tests

- If a change adds a behavior or a diagnostic, add a fixture under `tests/fixtures/<stage>/`.
- If a change alters a CLI command's arguments, exit codes or written files, add a test to `tests/cli.rs`.
- A regression test is a test that fails without the fix it covers.
- If a change fixes a bug, add a regression test.
- A regression test MAY be a fixture, a `tests/cli.rs` test, or a unit test.
- Before you commit a fix, revert the fix alone and run its regression test. If the test passes, change the test until it fails without the fix.
- You MAY run `gh workflow run new-tests.yml -f pull_request=<number>` to learn which of a pull request's tests fail on its merge base. `gh run view <run id>` shows the result.
- That workflow does not replace the revert: a test that pins existing behavior passes on the merge base, and the workflow skips unit tests.
- A test MUST reach every condition and match arm that a change adds.
- Before you commit, negate each new condition and empty each new match arm, one at a time. Run the tests after each change. If every test still passes, add a test that fails on that change.
- A slow input is an input that runs longer than one second in a debug build.
- If a bug shows only on a slow input, unit-test the structure that caused the bug.
- If a fixture imports a module, put the module in a subdirectory: each top-level fixture file is an entry.
- Give fixtures fake data only.
- A run fixture MAY carry a `<fixture>.arguments` sidecar with one argument per line.
- The harness runs the compiled fixture with those arguments in the fixture's directory.
- A run fixture MAY carry a `<fixture>.stdin` sidecar. The harness writes the sidecar to the compiled fixture's standard input, which is empty otherwise.
- If a fixture writes to standard error or exits nonzero, its snapshot appends the standard error and the exit code.
- The standard error and the exit code follow the standard output in the snapshot.
- Create or update snapshots with `INSTA_UPDATE=always`.
- NEVER commit a snapshot that you have not read.
- If a change adds or grows a corpus entry, build the compiler with `cargo build --release`.
- Then time the entry's emission with `bash -c 'time cargo run --release --quiet -- emit <entry> > /dev/null'`.
- If that emission takes more than one second, shrink the entry.
- NEVER lower `MINIMUM_LINE_COVERAGE` or raise `MAXIMUM_CRAP` in `xtask/src/gate.rs` without the project owner's approval.
- NEVER hide code from `cargo xtask coverage`, such as by moving branches into a macro: the gate cannot see inside a macro.

### Dev shell

**Build, test and run every tool inside the dev shell.** The host `PATH` can carry other versions of `cargo` and `rustc`.

- If `direnv allow` has run in this repository, `.envrc` loads the dev shell through nix-direnv.
- Stage each new file by path with `git add` before any `nix` command: flake commands ignore files outside the git index.
- The dev shell runs at most two compiling `cargo` commands at once across every worktree. Set `RSLISP_BUILD_SLOTS` to change the limit.

### Commits

- Make each commit one logical change.
- Each commit MUST build. The `commit builds` job in CI checks every commit of a ready pull request.
- Commit only the paths of that change: name the paths in `git commit`, because the index can hold other staged files.

## Commands

**Run the working commands while you work.**

```sh
cargo fmt --all --check
cargo test <narrowest test>
```

**If `IN_NIX_SHELL` is set, run every command as written.**

**If `IN_NIX_SHELL` is not set, prefix every command that runs a dev shell tool, such as `cargo`, with `direnv exec .`.** If direnv is missing or refuses, stop and ask the project owner.

**NEVER prefix a command with `nix develop -c`.** In a dirty worktree, the prefix evaluates the flake again and copies the tracked files into `/nix/store`.

**Replace `<narrowest test>` with the narrowest row of this table that covers the change:**

| The change touches | `<narrowest test>` |
| --- | --- |
| a unit test in `src/` | `--lib <module::path>` |
| a unit test in `expand/src/` | `-p rslisp-expand --lib <module::path>` |
| one stage's fixtures | `--test stages <stage>_fixtures` |
| a corpus entry | `--test stages corpus_fixtures` |
| a CLI command | `--test cli <test name>` |
| a diagnostic code | `--test diagnostic_codes --test diagnostic_snapshots` |
| `xtask/` | `-p xtask` |

- To run one fixture of a stage, prefix the stage's command with `INSTA_GLOB_FILTER='**/<fixture file name>'`.

**CI posts the front end's instruction counts on each ready pull request and uploads its callgrind profiles.** If a ready pull request carries the change, read them from CI as `docs/pull-requests.md` says, and NEVER run `cargo xtask bench measure`. Otherwise, you MAY measure them on Linux with `cargo xtask bench measure`. To compare two checkouts, measure each with `--workspace <checkout> --save <file>`, then run `cargo xtask bench compare <before> <after>`.

**NEVER run these commands locally:** CI does the same work.

- `cargo test` without a test target
- `cargo test --test cli` without a test name
- `cargo clippy --workspace`
- `cargo build --release` to probe a pull request's unedited code: download the binaries, as `docs/pull-requests.md` says
- `git rebase -x` with a build command, or another loop that builds each commit
- `cargo xtask coverage`
- `nix flake check`
- `nix build`, except the command below

**If a change touches what a small flake check checks, build that check alone with `nix build --no-link .#checks.<system>.<name>`.** Every flake check except `default` and `source-format`, which builds `default`, is small.

**The `source-format` flake check fails on any `.rslisp` file under `prelude/`, `examples/` or `tests/fixtures/` that `rslisp fmt --check` would change.** The check skips `tests/fixtures/read/`, `tests/fixtures/format/` and `tests/fixtures/expand/unreadable/`.

**A change is done when CI passes on its pull request.** The `cargo (macos-latest)` job runs only on pushes to `main`.

## Reference files

**Each reference file holds rules that apply only to one kind of task. Before you start that task, read the file.** A reference file binds as fully as this file does.

| Reference file | Read the file before you |
| --- | --- |
| `docs/language.md` | change the language or the Rust it emits |
| `docs/diagnostics.md` | add or change an error or a warning |
| `docs/pull-requests.md` | open a pull request, push to one, review one, or fix a CI failure |
| `rfcs/README.md` | write a design, or change a rule that an RFC or decision records |
