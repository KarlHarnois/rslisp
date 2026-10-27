# rslisp agent guide

This file and the reference files it names are the living reference for working in this repository. If a change makes a statement in any of them false, update the statement in the same commit.

## Terms

| Term | Meaning |
| --- | --- |
| Project owner | The person you work for in this repository |
| Kernel form | A form the compiler knows, listed by `KernelForm` in `src/core/kernel.rs` |
| Prelude | The language's library under `prelude/`, written in the language and embedded in the compiler |
| Stage | One step of the compiler, such as reading, expansion or emission |
| Fixture | A source file under `tests/fixtures/` whose output an insta snapshot records |
| Corpus entry | A top-level fixture under `tests/fixtures/corpus/`, sized to measure the compiler's performance |
| Working commands | The three commands at the top of the Commands section |

## Principles

**If no rule covers a case, choose what these principles favor.** If two principles conflict, the earlier principle wins.

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
- NEVER put an abbreviation, a number series, or a noise word in a name.
- If a value's meaning is not obvious, make the value a named constant.
- In Rust code, take the language's name and file extension from `Language` in `src/language.rs`: `rslisp` is a placeholder name.

### Stages

- A stage depends only on earlier stages. NEVER make a stage depend on a later one.
- A stage reports problems as `Diagnostic` values. NEVER print from a stage.
- Only `src/cli/` prints.
- Only `src/cli/`, `src/build/`, `ModuleSources` and `xtask/` touch the file system or run processes.
- Write every diagnostic's wording in the stage's problems type, such as `ExpandProblems` or `TypeProblems`.

### Tests

- If a change adds a behavior or a diagnostic, add a fixture under `tests/fixtures/<stage>/`.
- If a change alters a CLI command's arguments, exit codes or written files, add a test to `tests/cli.rs`.
- If a fixture imports a module, put the module in a subdirectory: each top-level fixture file is an entry.
- Give fixtures fake data only.
- A run fixture MAY carry a `<fixture>.arguments` sidecar with one argument per line.
- The harness runs the compiled fixture with those arguments in the fixture's directory.
- When a fixture writes to standard error or exits nonzero, its snapshot appends the standard error and exit code after the standard output.
- Create or update snapshots with `INSTA_UPDATE=always`.
- Keep the front end of each corpus entry at one second or less in a release build.
- NEVER commit a snapshot that you have not read.
- NEVER lower `MINIMUM_LINE_COVERAGE` or raise `MAXIMUM_CRAP` in `xtask/src/gate.rs` without the project owner's approval.
- NEVER hide code from `cargo xtask coverage`, such as by moving branches into a macro: the gate cannot see inside a macro.

### `nix develop`

**Build, test and run every tool inside `nix develop`.** The host `PATH` can carry other versions of `cargo` and `rustc`.

- If `direnv allow` has run in this repository, `.envrc` loads the same dev shell through direnv.
- Run the working commands as written. Their `nix develop -c` prefix makes them work without direnv.
- Stage each new file by path with `git add` before any `nix` command: flake commands ignore files outside the git index.

### Commits

- Make each commit one logical change.
- Each commit MUST build.
- Commit only the paths of that change: name the paths in `git commit`, because the index can hold other staged files.

## Commands

**Run the working commands while you work.**

```sh
nix develop -c cargo fmt --all --check
nix develop -c cargo clippy --workspace --all-targets -- -D warnings
nix develop -c cargo test <narrowest test>
```

**Replace `<narrowest test>` with the narrowest row of this table that covers the change:**

| The change touches | `<narrowest test>` |
| --- | --- |
| a unit test in `src/` | `--lib <module::path>` |
| one stage's fixtures | `--test stages <stage>_fixtures` |
| a corpus entry | `--test stages corpus_fixtures` |
| a CLI command | `--test cli <test name>` |
| a diagnostic code | `--test diagnostic_codes --test diagnostic_snapshots` |
| `xtask/` | `-p xtask` |

- To run one fixture of a stage, prefix the stage's command with `INSTA_GLOB_FILTER='**/<fixture file name>'`.

**NEVER run these commands locally:** each compiles the whole suite or rebuilds everything on all cores, and CI runs them.

- `cargo test` without a test target
- `cargo xtask coverage`
- `nix flake check`
- `nix build`, except the command below

**If a change touches what a small flake check checks, build that check alone with `nix build --no-link .#checks.<system>.<name>`.** Every flake check except `default` is small.

**A change is done when CI passes on Linux and macOS.**

## Reference files

**Each reference file holds rules that apply only to one kind of task. Before you start that task, read the file.** A reference file binds as fully as this file does.

| Reference file | Read the file before you |
| --- | --- |
| `docs/language.md` | change the language or the Rust it emits |
| `docs/diagnostics.md` | add or change an error or a warning |
| `docs/pull-requests.md` | open a pull request, push to one, or fix a CI failure |
| `rfcs/README.md` | write a design, or change a rule that an RFC or decision records |
