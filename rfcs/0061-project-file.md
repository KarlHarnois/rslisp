# RFC 0061: Project file, crate dependencies and libraries

**Status:** Accepted
**Date:** 2026-11-07

## Problem

A program cannot depend on a crate or on another rslisp project. `RustCompiler` in `src/build/mod.rs` runs rustc on one emitted file, with no `--extern` and no Cargo. A tool cannot find a project's root ([#294](https://github.com/KarlHarnois/rslisp/issues/294)), and sharing dependency builds across worktrees ([#325](https://github.com/KarlHarnois/rslisp/issues/325)) waits on dependencies existing at all.

The language cannot solve this from a program: a `use` reaches only files under one root, and no form can add a crate to rustc's command line.

Two later RFCs build on this one: a program interpreter, and compiled crate bindings that the interpreter loads for a live REPL. This RFC fixes the rules they rely on and designs neither.

## Decision

The project owner decided these 12 points on 2026-11-07. The owner decided the 12 questions under [Options](#options) on the same date, as [Decided questions](#decided-questions) records.

1. **A project is a directory with a `project.rslisp` file.**
2. **The file is data in the language's syntax, read without expansion.** Nothing in it expands or runs. An unknown key is an error that suggests the closest key. The reasons: TOML adds a second syntax, and a hand-written `Cargo.toml` exposes Cargo's whole surface.
3. **Crate dependencies are named exactly as on crates.io.** A name is never mangled.
4. **rslisp libraries are source dependencies, listed apart from crates.** The compiler reads a library's source and checks it with the program, so inference, ownership modes and macros work across the boundary as they do within one project. Compiled libraries would stop inference of modes and concrete types at the boundary.
5. **A program with crate dependencies builds through Cargo.** A program without them keeps the direct rustc path. Cargo resolves, downloads and builds, so the compiler takes on no resolution, features or build scripts.
6. **Cargo's `Cargo.lock` is the lockfile for crates.** It lives at the project root.
7. **A git library pins an exact commit,** so a build is reproducible without a lockfile of the language's own.
8. **The root is found by walking up** from the entry file or the current directory to the nearest `project.rslisp`. An explicit `--root` wins. Without a project file, commands keep today's rule.
9. **Every build shares one target directory per user.** It is `CARGO_TARGET_DIR`, by default `$XDG_CACHE_HOME/rslisp/target`. Git libraries are cloned once into a shared store keyed by commit.
10. **`rslisp new <name>`, `rslisp add <crate> [--features ...] [--dev]` and `rslisp remove <crate>`** write and edit the file through the formatter.
11. **A crate dependency adds no names.** A program reaches a crate only through forms with a `rust` clause or template. Binding a crate's API is library work in the language, not compiler work.
12. **Dependencies are visible to program code only, never to macro code,** so a macro's output depends only on its input forms, the prelude and libraries. This keeps expansion deterministic and lets [#326](https://github.com/KarlHarnois/rslisp/issues/326) cache expanded modules. Macros that run crate code, as Rust's procedural macros do, are out of scope.

### Decided questions

The project owner took option A of questions 1 to 12 on 2026-11-07.

1. **A dependency entry.** A dependency is a list: the name first, an optional version string, then keyword pairs, as `(serde "1.0" :features ("derive"))` and `(hints :path "../hints")`.
2. **Features.** Feature names are strings.
3. **A crate's name.** A crate's name stays as crates.io writes it, and this is the one recorded exception to the kebab-case rule.
4. **The two build paths.** Both paths read one plan, the Cargo path runs `cargo rustc -- --cap-lints allow`, and a test in `tests/cli.rs` compares the two on one program.
5. **`check`.** `check` reads the project file and not the crate graph.
6. **`rslisp test`.** `rslisp test` builds one crate, as a `[[test]]` target with `harness = false` and no bin.
7. **The build directory.** In every project, on both paths, the build directory is `<root>/target/rslisp/<name>/`, and each generated manifest holds an empty `[workspace]` table.
8. **The shared target directory.** The binary's name holds a hash of the project root, and the manifest's package name is a constant. The owner accepts that one user's Cargo builds run one at a time.
9. **The dev shell's cargo wrapper.** `rslisp` runs `cargo` from `PATH` and knows nothing of build slots.
10. **What `use` resolves from.** The project root is where the file lives, and `use` resolves from the source root, the directory of `:entry`, or of `:lib` inside a library.
11. **`--root`.** `--root` stays on `eval` and `repl` and names the source root. Other commands walk up from their file, and in a project they take their file as optional and default to `:entry`.
12. **`rslisp test` and the project file.** `rslisp test` skips the project file, searches a test directory before the source root, and the runner calls only the tests of the project's own modules.

### The file

This example applies decisions 1 to 3 of the questions and the full `:rev` of the smaller details.

```lisp
(project browser
  :version "0.1.0"
  :entry "src/main.rslisp"
  :edition 2026

  (libraries
    (hints :path "../hints")
    (keymap
      :git "https://github.com/KarlHarnois/keymap"
      :rev "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678"))

  (dependencies
    (cef "144.0")
    (serde "1.0" :features ("derive")))

  (dev-dependencies
    (proptest "1.5")))
```

- The file holds exactly one `project` form. Its first part is the name.
- Only the name is required. `:entry` defaults to `src/main.rslisp`, `:version` to `"0.1.0"`, and `:edition` to the compiler's current edition.
- `:lib` names a library's root module and defaults to `src/lib.rslisp`. A project may have both `:entry` and `:lib`. A library has no `main`.
- `dev-dependencies` are available to `rslisp test` only.

### Libraries

- `(use hints)` imports the library's root module. `(use hints/labels label)` imports one of its modules.
- A library's name takes precedence over a local directory of the same name, and the clash is reported.
- A library's `export` lists are its public interface. Modules it never exports from are private to it.
- A library's macros are usable by its importers.
- A library's crate dependencies join the program's manifest, and Cargo resolves their versions together.
- Two libraries with the same name at different sources are an error.

### Using a crate

A crate-backed form spells its path in Rust, as any form with a `rust` clause does. A crate name with a hyphen takes Rust's underscore in a path, as `serde_json::Value` does for `serde-json`, because that is the path rustc reads.

```lisp
(deftype browser (rust "cef::Browser"))

(def open (rust "cef::Browser::open({0})" (fn ((ref str)) browser)))

(def main (fn ()
  (open "https://example.com")
  (println "opened")))
```

```rust
fn main() {
    cef::Browser::open("https://example.com");
    println!("opened")
}
```

`deftype` and the `(rust "Path")` clause are the spelling of RFC 0059's decision 3, and they land with that RFC's batch 5. Until then a program writes `rust-type` and `rust-trait`.

**A form is crate-free when nothing it reaches names a crate path.** A template names a crate path when a Rust token outside a string literal, followed by `::`, equals the Rust name of a dependency. A form that reaches a crate-backed type counts too, so `"{0}.title()"` on a `browser` is not crate-free. `src/emit/template.rs` already reads templates, and the rule sits beside it.

The later interpreter uses this rule in program code: a form that is not crate-free is compiled, not interpreted. Nothing in this RFC assumes that each input runs in a fresh process, so a later live REPL can keep crate objects across inputs.

### Macro code

Decision 12 holds today for every Rust form, with or without a crate, so it needs no new check. On `main`, a macro that calls `(def cef-version (rust "cef::version()" (fn () i64)))` reports:

```text
error[L0306]: `cef-version` belongs to the program, so macro code cannot run it
```

The later interpreter must keep this report for a form that is not crate-free.

### Building

With no crate dependencies, `build` emits one file and calls rustc, as today.

With crate dependencies:

1. Write `Cargo.toml` and `src/main.rs` into the build directory. The manifest's dependencies translate the project file and every library's crate dependencies, and a relative `:path` becomes absolute.
2. Copy the project's `Cargo.lock` into the build directory, if it exists.
3. Run `cargo rustc --bin <name> --message-format=json -- --cap-lints allow`, with `--release` for a release build. Question 4 explains why this is not `cargo build`.
4. Read rustc's messages from the `compiler-message` records on standard output and pass them to `RustcDiagnosticMapper`, which reads the same JSON that rustc writes on the direct path. A message from another package reports as a dependency error that names the crate.
5. Copy the binary from the `executable` field of the `compiler-artifact` record to the output path, and `Cargo.lock` back to the project root.

Cargo writes a failure to resolve or to fetch as text on standard error, with no JSON record and exit code 101. The prototype asked for `itoa = "99.0"` and got that. The command prints the text and exits with the environment status.

The build directory is never edited by hand and may be deleted at any time.

**Preparing dependencies is a separate step from compiling the program.** The step builds the manifest with a stub `fn main() {}`. The prototype then built the real `main.rs`, and each dependency reported `fresh`. A REPL session, or a test or `run` that first falls back to rustc under the later interpreter, takes this step once. Each crate-reaching input then compiles only its own code against the prepared crates and runs in its own process. Values do not persist across such inputs until a later RFC.

### Later work

- `:tag` for git libraries, with a small lockfile that records each tag's commit.
- Version ranges and resolution for libraries.
- A registry.
- Workspaces.

## Options

Each question came out of comparing the owner's draft with `main` at `813d5d8e`, or out of a prototype. Questions 2, 8 and 10 came up while writing this RFC, and the others are the questions the draft and RFC 0059 leave open. Prototypes ran under `target/scratch/` with cargo 1.95.0 and rustc 1.95.0, on fake local crates and the crate `itoa`.

### Question 1: what a dependency entry looks like

The draft writes `:hints (:path "../hints")`. RFC 0059's decision 5 makes `(:name p)` a field access, so a keyword at the head of a list reads as one, and `:path` here would read `"../hints"`.

| Option | Source | Result |
| --- | --- | --- |
| A, an entry is a name and fields | `(hints :path "../hints")` | The shape of a variant with named fields in RFC 0059: `(labeled :text string :size f64)`. A field is a keyword, and the thing defined is a bare name |
| B, a named source | `:hints (path "../hints")` | The field stops being a keyword, and `path` reads as a call |
| C, a form per entry | `(library hints :path "../hints")` | Clear, and repeats the head that a section states |
| D, the draft | `:hints (:path "../hints")` | Reads as a field access |

**Recommendation: A.** Decision 5 reads "a field is always a keyword, and a binding is always a bare name", and an entry binds a name. An entry's first item after the name, when it is a string, is the crate's version, so `:version` is not a key and each fact has one spelling.

The list of features has the same fault: `:features (derive)` reads as a call of `derive`, a form the language has. Question 2 covers it.

### Question 2: how a list of features is written

| Option | Source | Result |
| --- | --- | --- |
| A, strings | `:features ("derive" "std")` | A list headed by a string cannot read as a call, as RFC 0059 tells a `rust` clause from a method because no type is a string. A feature is Cargo's text, as a version is, and it may hold `/` or `dep:` |
| B, bare names | `:features (derive std)` | The draft. Reads as a call of `derive` |

**Recommendation: A.**

### Question 3: spelling a crate key

`docs/language.md` holds five naming rules, and rule 1 makes every name kebab-case. `Inflector` and `tokio_util` break it, and decision 3 says that a name is never mangled.

| Option | Source | Result |
| --- | --- | --- |
| A, the name as written | `(tokio_util "0.7")` | One exception, recorded beside the rules. The name is Cargo's, not the language's: it binds nothing in a program, and no source form refers to it |
| B, a string | `("tokio_util" "0.7")` | No exception, and two spellings of an entry, one for libraries and one for crates |
| C, kebab-case with translation | `(tokio-util "0.7")` | Mangling, which decision 3 rules out, and the file stops showing the name that crates.io and the crate's documentation print |

**Recommendation: A.** The reader takes these names as symbols: `rslisp fmt` lays out `(Inflector "0.11")` and `(tokio_util "0.7")` unchanged on `main`. A library's name follows the language's rule, because `use` reads it.

### Question 4: the two build paths

A direct build and a Cargo build must give the same program. The emitted Rust is one `EmittedCrate`, so the difference can only come from the command. A prototype compared them:

| Fact | Direct rustc | Plain Cargo | What this RFC does |
| --- | --- | --- | --- |
| Rust edition | `RustCompiler::EDITION` | the manifest's | The manifest reads the same constant |
| Lints | `--cap-lints allow` | not capped | `cargo rustc -- --cap-lints allow` |
| Debug info in a debug build | none | `-C debuginfo=2` | `[profile.dev] debug = false` |
| Optimization in a release build | `-C opt-level=3` | `-C opt-level=3` | Nothing |
| Overflow checks and debug assertions | on at opt-level 0, off at 3 | the same | Nothing |
| Output name | the entry's stem, so `main` for `src/main.rslisp` | the bin's name | In a project, both write the project's name |
| Crate name | the same | the bin's name, with question 8's hash | The two differ. Only `module_path!` and `type_name` show it, and neither the prelude nor the emitter writes them |

The second row decides real behavior, because `--cap-lints allow` also turns off deny-by-default lints. `let x: i64 = 1 / 0` builds on the direct path and panics at run time. `cargo build` refuses it with `unconditional_panic`. `cargo rustc -- --cap-lints allow` built it, the binary panicked as the direct one did, and the extra flag left each dependency `fresh`. So this RFC runs `cargo rustc`, where the draft wrote `cargo build`. A bare `cargo build` would also split a program's meaning on whether it has an unrelated dependency.

| Option | What CI checks |
| --- | --- |
| A, one plan and one comparison | `RustCompiler` and the manifest writer read one value for edition, profile and lints, so a new flag has one place to go. A test in `tests/cli.rs` builds one program in a debug and a release build, with and without a fake local crate that the program does not use, and compares standard output, exit code and standard error. The program covers an overflow, a division by a literal zero and an exit code |
| B, every run fixture both ways | All 519 top-level run fixtures through Cargo as well. Strong, and it spends CI time on one difference: the flags |
| C, always Cargo | Decision 5 rules this out, and a program with no crates would pay for Cargo |

**Recommendation: A.** The fake crate is a `:path` dependency, so the test needs no network and runs in a Nix sandbox. A panic message prints the emitted file's path, which differs between the paths, so the comparison replaces that path. The test sets `XDG_CACHE_HOME` to a temporary directory, so it shares no state.

### Question 5: whether `check` needs the crate graph

`check` reads a `rust` template as unchecked text. On `main`, a program with `(rust "cef::version()" (fn () i64))` and no `cef` passes `check`, and `build` reports `E0433` mapped to the call.

| Option | Result |
| --- | --- |
| A, `check` reads the project file and not the graph | No Cargo and no crates.io in the edit loop. The check still reports a bad project file, and a git library that is not in the store yet is fetched, because its source is part of the program |
| B, `check` runs `cargo metadata` | Learns versions and features and nothing about types, because a template stays text. Needs the network on a first run |
| C, `check` runs `cargo check` on the emitted Rust | Catches everything, and puts rustc in the loop that [#323](https://github.com/KarlHarnois/rslisp/issues/323) means to free from it |

**Recommendation: A.** It takes one of the two answers that #323 leaves open: a template's declared type is the contract, and when rustc disagrees it blames the template. Each such error stays visible at `build`, mapped to source. A later RFC can add a check of crate-backed forms if the gap hurts.

### Question 6: how `rslisp test` combines with Cargo

`rslisp test` builds one program whose `main` is `run-tests`, then `TestRun` reads its records from standard output. That program must see `dev-dependencies`, and Cargo gives those only to tests, examples and benches.

| Option | Result |
| --- | --- |
| A, a `[[test]]` target with `harness = false`, and no bin | Builds with `cargo rustc --test <name>`, sees both kinds of dependency, and `executable` names the binary. The `fn main` that `run-tests` emits runs as it does today, with its arguments. `Cargo.lock` came out byte-identical to the bin manifest's |
| B, the bin and a `[[test]]` target in one manifest | Cargo builds the bin too, so `rslisp test` would fail on a program whose `main` does not compile for a reason a test does not reach |
| C, merge `dev-dependencies` into `[dependencies]` for tests | One lock, written by two manifests that disagree, so `build` and `test` rewrite the root's `Cargo.lock` in turn |
| D, an `[[example]]` target | Works, and the name states the wrong thing to anyone who reads the manifest |

**Recommendation: A: one crate, as a test target.** Both manifests carry both dependency sections, and only the target differs. With `[dev-dependencies]` in the bin manifest too, a bin that names a dev crate gets `E0433` from rustc, mapped to source. The prototype reported that error with byte offsets on `src/main.rs`, which is what `RustcDiagnosticMapper` reads.

### Question 7: the build directory

`Terminal::BUILD_DIRECTORY` is `target/rslisp`, relative to the working directory, and `BuildDirectory` mirrors the entry's directory under it. `eval`, `repl` and `test` build in a `TemporaryDirectory`.

| Option | Result |
| --- | --- |
| A, in a project, one fixed directory `<root>/target/rslisp/<name>/`, with `build`, `test` and `eval` under it | Cargo's own files stay in the shared target. The sources stay inside the project, so a project's `.cargo/config.toml` applies. A command gives one result from any working directory |
| B, keep today's directories | Two working directories make two build directories for one project, and a temporary directory loses its `Cargo.lock` |

**Recommendation: A,** in every project, on both build paths. Outside a project nothing changes.

Each generated manifest needs an empty `[workspace]` table. Without it, a build directory under a Cargo workspace fails: `current package believes it's in a workspace when it's not`. The prototype reproduced this, and this repository's own `Cargo.toml` is such a parent.

### Question 8: the shared target directory

Decision 9 holds, with one requirement the draft did not state. Cargo's hash for a path package is relative to its workspace root, so two worktrees of one project get **the same hash in the shared directory**. In the prototype, two worktrees built one package name, one after the other, and the second reported `fresh` and left the first worktree's binary in place. It ran `w1 41` for the source of `w2`, because Cargo compares file times, and the source of `w2` was older than the first build. A generated `main.rs` that is rewritten on each build hides this and rebuilds on every switch, and a binary copied after Cargo exits can still be another worktree's.

| Option | Result in the prototype |
| --- | --- |
| A, a bin name with a hash of the project root's canonical path, as `browser-1f3a9c2e` | Each worktree got its own binary and its own rebuild. Dependencies stayed `fresh` across worktrees, and `Cargo.lock` stayed byte-identical, because a lock records packages and not targets |
| B, a package name with that hash | Also separates them, and `Cargo.lock` then differs in every worktree |
| C, one target directory per project | Solves it, and reverses decision 9 |

**Recommendation: A.** The manifest's package name is a constant, the language's name followed by `-program`, because a project named as its own dependency otherwise fails with `package collision in the lockfile`. The binary is copied to its output path, so users never see the hash.

```toml
[package]
name = "rslisp-program"
version = "0.1.0"
edition = "2024"
publish = false
autobins = false
autotests = false
autoexamples = false
autobenches = false

[[bin]]
name = "browser-1f3a9c2e"
path = "src/main.rs"

[workspace]

[profile.dev]
debug = false

[dependencies]
cef = "144.0"
serde = { version = "1.0", features = ["derive"] }

[dev-dependencies]
proptest = "1.5"
```

The hash also keeps the directory bounded, because a stable name per project replaces a new name per temporary directory.

A user's own `CARGO_TARGET_DIR` wins over the default, as the draft's "by default" allows.

**Cost: one Cargo at a time per target directory.** Cargo holds a lock on the directory for the whole build, so a second concurrent build prints `Blocking waiting for file lock on artifact directory` and waits. In the prototype a 1.9 second build next to another took 3.6 seconds. Builds of different projects therefore run one after another for one user. This trades latency for the CPU and disk that #325 set out to save, and it fits the plan that local work checks and tests without rustc. A target per project would not wait, and would give up that sharing.

### Question 9: the dev shell's cargo wrapper

The dev shell puts a wrapper on `PATH` that limits compiling `cargo` commands to `RSLISP_BUILD_SLOTS`.

| Option | Result |
| --- | --- |
| A, run `cargo` from `PATH` and know nothing of slots | `cargo rustc` takes a slot, as it should. Under `cargo test` or `cargo run`, the wrapper's `RSLISP_BUILD_SLOT_HELD=1` reaches the nested `rslisp` and its `cargo`, which skips the wrapper, so nothing deadlocks. `add`, `remove` and `fetch` take no slot |
| B, have `rslisp` read `RSLISP_BUILD_SLOTS` | Puts a detail of this repository's dev shell into the compiler that every user runs |

**Recommendation: A.** The wrapper's slot files sit in `$HOME/.cache/rslisp/build-slots`, beside the shared target and apart from it. Two costs remain. A direct rustc build takes no slot, as today. A build that waits on Cargo's lock of question 8 holds its slot while it waits, so in the dev shell one blocked build can occupy a slot that a build in another target directory could use.

### Question 10: what `use` resolves from

The draft says that the project file's directory is the root, and also that the entry is `src/main.rslisp` and that `(use hints/labels label)` reaches a library's module. Those do not fit one root: `src/labels.rslisp` beside the entry would be `(use src/labels)`, while `docs/language.md` says `use` loads relative to the entry file's directory.

| Option | Result |
| --- | --- |
| A, two roots | The project root is where the file lives, and tools and builds read it. The source root is the directory of `:entry`, or of `:lib` inside a library, and `use` resolves from it. `(use labels)` keeps working when a program moves into a project |
| B, one root | `use` resolves from the project root, so every import inside the project starts with `src/`, and a library's `hints/labels` needs its own rule anyway |

**Recommendation: A.** `ModuleSources` holds one root today, and it gains the table of library names: the first segment of a module path picks a library, and the rest resolves from that library's source root. Inside a library, `use` works as in any project, and the module's path in the program takes the library's name as its prefix. `prelude`, `kernel` and `code` are reserved library names.

### Question 11: `--root`

Only `eval` and `repl` have `--root` on `main`. `eval` takes an `Option<PathBuf>` and defaults to the file's directory. `repl` has `default_value = "."`, so it cannot tell an explicit flag from the default. `run`, `check`, `build` and `test` have none.

| Option | Result |
| --- | --- |
| A, `--root` stays on `eval` and `repl`, and names the source root | `repl` takes an `Option`, and with no flag it walks up from the working directory. Every other command walks up from its file |
| B, add `--root` to every command | A flag no one needs, because walking up covers those commands |

**Recommendation: A.** With the flag, a command still walks up to find the project's crates and libraries, because `--root` answers where `use` starts, not which project this is. In a project, `build`, `run`, `check`, `types`, `expand` and `emit` take their file as optional and default to `:entry`. Outside one, a missing file is a usage error.

### Question 12: `rslisp test` and the project file

`deftest` and the runner need three changes that no other question covers.

| On `main` | Problem | Recommendation |
| --- | --- | --- |
| `TestEntries::collect_modules` takes every `.rslisp` file under a directory | `rslisp test .` in a project fails with `L0201` at `project` | Skip the project file. `rslisp fmt` keeps taking it |
| A directory is the root of `use` | `tests/parse.rslisp` cannot reach `src/parser.rslisp` | In a project, a test directory is searched first, then the source root |
| `run-tests` calls every test of `(program-modules)` | A test run would also run every library's tests, which may need that library's `dev-dependencies`, which never join a dependent's manifest | The runner calls the tests of the project's own modules. Every test still expands and checks, and an uncalled test emits nothing |

**Recommendation: all three.** The emitter writes only what `main` reaches, so a library's dev crate never reaches a dependent's emitted Rust.

### Smaller details the build settles

None of these changes a decision.

- **`:rev` is a full 40-character commit.** The draft's example has 7 characters, which is not an exact commit. Fetching one fails, `couldn't find remote ref`, and a store keyed by commit needs one key per commit. A shorter value is an error.
- **`:edition`** is the language's edition, not Rust's, and `:edition 2026` writes no Rust edition. The compiler has one edition today, so any other value is an error that names it.
- **`:optional`** passes through to Cargo with Cargo's meaning, and the project file has no features of its own to turn such a crate on. Keep it for parity, or drop it until the file has them.
- **`:path` in a crate entry** resolves from the file that wrote it, so a library's entry resolves from the library.
- **The layout of the file.** `rslisp fmt` reads the file today, keeps comments and blank lines, and indents past `(project browser`, because `project` has no shape. A shape needs a binding, as `L0201` on a misspelled shape shows, so a prelude module states the shapes of `project` and its sections with `:one-per-line`, and `fmt` reads that module for this one file name.
- **The file's name.** Rust code builds it from `Language::EXTENSION`. A module of a program may no longer be named `project` at the root. No file in this repository is.
- **`rslisp add`** gets a version from `cargo add` on a scratch manifest, so the resolved version is Cargo's choice.
- **Diagnostics.** The project file's errors take a new `L07xx` range, written in a `ProjectProblems` type. `docs/diagnostics.md` gains the row.
- **`cargo` missing** is an environment error that names what the project needs it for. `rslisp check` does not need it.
- **Writing `main.rs` only when it changed** lets an unchanged project skip rustc, which the direct path cannot do. The hash of question 8 makes this safe.

## Recommendation

Take option A of each question. In short: a dependency entry is `(name "version" :field value)`, features are strings, and a crate's name stays Cargo's. Both build paths read one plan, and CI compares them on one program. `check` stays away from Cargo. `rslisp test` builds a `harness = false` test target. A hash in the bin's name keeps worktrees apart in the shared target.

## Rejected options and why

- **Driving rustc with `--extern` against prebuilt rlibs:** a faster start, and the compiler takes on resolution, features and build scripts.
- **An own dependency tool and registry:** large, and duplicates crates.io.
- **TOML, or a hand-written `Cargo.toml`:** decision 2.
- **Libraries as compiled crates:** decision 4.
- **Macros that run crate code:** decision 12.
- **`cargo build` as the draft wrote it:** it rejects deny-by-default lints that the direct path allows, as question 4 shows.
- **A keyword at the head of a dependency's list:** it reads as a field access under RFC 0059.

## Rule

The build adds a Projects section to [docs/language.md](../docs/language.md), and changes [Modules](../docs/language.md#modules), [Tests](../docs/language.md#tests), [Rust interop](../docs/language.md#rust-interop) and [Names](../docs/language.md#names).
