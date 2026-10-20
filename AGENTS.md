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
| The Commands | The five commands in the Commands section |

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

### Rust's performance and model

- NEVER emit a clone or an allocation that the source does not write.
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
- Only `src/cli/`, `src/build/` and `ModuleSources` touch the file system or run processes.
- Write every diagnostic's wording in the stage's problems type, such as `ExpandProblems` or `TypeProblems`.

### Tests

- If a change adds a behavior or a diagnostic, add a fixture under `tests/fixtures/<stage>/`.
- If a change alters a CLI command's arguments, exit codes or written files, add a test to `tests/cli.rs`.
- If a fixture imports a module, put the module in a subdirectory: each top-level fixture file is an entry.
- Give fixtures fake data only.
- Create or update snapshots with `INSTA_UPDATE=always`.
- NEVER commit a snapshot that you have not read.

### `nix develop`

**Build, test and run every tool inside `nix develop`.** The host `PATH` can carry other versions of `cargo` and `rustc`.

- If `direnv allow` has run in this repository, `.envrc` loads the same dev shell through direnv.
- Run the Commands as written. Their `nix develop -c` prefix makes them work without direnv.
- Stage each new file by path with `git add` before any `nix` command: flake commands ignore files outside the git index.

### Commits

- Make each commit one logical change.
- Each commit MUST build.
- Commit only the paths of that change: name the paths in `git commit`, because the index can hold other staged files.

## Commands

```sh
nix develop -c cargo fmt --check
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo test
nix build .#
nix flake check
```

**A change is done when the Commands pass.** CI runs them on Linux and macOS. If one of the Commands also fails without the change, stop and show its output to the project owner.

## Reference files

**Each reference file holds rules that apply only to one kind of task. Before you start that task, read the file.** A reference file binds as fully as this file does.

| Reference file | Read the file before you |
| --- | --- |
| `docs/language.md` | change the language or the Rust it emits |
| `docs/diagnostics.md` | add or change an error or a warning |
