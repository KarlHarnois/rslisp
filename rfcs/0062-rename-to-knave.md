# RFC 0062: Rename the language to knave

**Status:** Accepted
**Date:** 2026-11-07

## Problem

`rslisp` is a placeholder name. The README and `docs/language.md` say so, and `AGENTS.md` keeps Rust code reading the name and extension from `Language` in `expand/src/language.rs` so that the rename stays cheap. The repository already lives at `github.com/knave-lang/knave`, but the crate, the binary, the extension, the dev shell and every document still say `rslisp`.

`Language` does not reach every place the name appears. Cargo package names, the flake, environment variables, shell scripts, file names, snapshot names and prose cannot read a Rust constant, and a few Rust strings restate the name instead of reading it. The rename also moves every source file and nearly every snapshot, so it collides with any open pull request that touches a fixture.

## Decision

The project owner decided these points on 2026-11-07.

1. **The language is named knave,** in the lineage of Planner, Conniver, Scheme, Guile and Racket.
2. **Source files use the extension `.knv`.**
3. **The command-line binary is `knave`.**
4. **The old name and extension stop working at once.** Before 1.0 the owner is the only user, so the build gives no transition period and no migration aid. The build renames the repository's own files.
5. **The Cachix cache stays `rslisp.cachix.org`.** Renaming the cache is later work, outside this RFC's build. [Question 2](#question-2-the-cachix-cache) records the options.

### Decided questions

The project owner decided the five questions under [Options](#options) on 2026-11-07, each as [Recommendation](#recommendation) proposed.

1. **Every place the name appears changes,** as [Option 1A](#option-1a-rename-every-place) says, crate names and dev shell variables included.
2. **The cache stays,** as [Option 2B](#option-2b-keep-rslispcachixorg) says.
3. **Accepted RFCs stay as history,** as [Option 3A](#option-3a-leave-accepted-rfcs-as-history-rewrite-live-text) says. Live text changes, and `docs/language.md` names the old name once.
4. **The name changes first, then the extension by script in a quiet window,** as [Option 4B](#option-4b-the-name-first-then-the-extension-by-script-in-a-quiet-window) says.
5. **The owner makes the changes outside the repository after stage 2 merges,** as [Question 5](#question-5-changes-outside-the-repository) lists them. The main checkout moves only after stage 2, when no session runs.

## Options

All counts come from `git grep` and `git ls-files` on `main` at `cefd5257`.

### Question 1: every place the name appears

`git grep -i rslisp` finds 3,887 mentions in 1,864 files. Of these, 21 files are RFCs ([Question 3](#question-3-accepted-rfcs)), and the rest split into the name and the extension.

**Places that read `Language` already, and follow `Language::NAME` with no other change:**

- the CLI's program name, through `name = Language::NAME` in `src/cli/mod.rs`, which the help prints as `Usage: rslisp`
- the prompt history directory, `$XDG_STATE_HOME/rslisp/history` or `~/.local/state/rslisp/history`, in `src/cli/prompt_history.rs`
- the temporary directory prefix in `src/cli/temporary_directory.rs`
- the test runner's record marker in `src/cli/value_marker.rs`
- the rustfmt time limit variable, `RSLISP_RUSTFMT_TIME_LIMIT_MILLISECONDS`, which `src/build/formatting.rs` builds from the name
- the extension checks in `src/cli/fmt.rs`, `src/cli/test_entries.rs`, `src/cli/doc.rs`, `expand/src/modules/sources.rs`, `src/types/typed_source.rs` and `tests/support/formatting.rs`, which follow `Language::EXTENSION`
- the bench corpus in `xtask/`, which reads each checkout's extension from that checkout's `expand/src/language.rs`

**Places that state the name and cannot read `Language`:**

| Place | Today | After |
| --- | --- | --- |
| Root package in `Cargo.toml` and `Cargo.lock` | `rslisp` | `knave` |
| Expansion package in `expand/Cargo.toml`, `Cargo.toml` and `Cargo.lock` | `rslisp-expand` | `knave-expand` |
| Rust paths in `src/main.rs`, `src/lib.rs`, `src/emit/mod.rs`, `build.rs`, `tests/stages.rs`, `tests/diagnostic_snapshots.rs`, `tests/support/` | `rslisp::`, `rslisp_expand::` | `knave::`, `knave_expand::` |
| Binary path in `tests/cli.rs` | `CARGO_BIN_EXE_rslisp` | `CARGO_BIN_EXE_knave` |
| Flake package in `flake.nix` | `pname`, `--package`, `wrapProgram $out/bin/rslisp`, `mainProgram` | `knave` |
| Flake and package descriptions | "rslisp (placeholder name)" | "knave" |
| Dev shell variables in `flake.nix` and `AGENTS.md` | `RSLISP_BUILD_SLOTS`, `RSLISP_BUILD_SLOT_HELD` | `KNAVE_BUILD_SLOTS`, `KNAVE_BUILD_SLOT_HELD` |
| Dev shell slot directory in `flake.nix` | `~/.cache/rslisp/build-slots` | `~/.cache/knave/build-slots` |
| GitHub linguist rule in `.gitattributes` | `*.rslisp` | `*.knv` |
| `source-format` check in `flake.nix` | `-name '*.rslisp'`, "run rslisp fmt" | `-name '*.knv'`, "run knave fmt" |
| Prelude docstring of `deftest` in `prelude/test.rslisp` | "`rslisp test` runs" | "`knave test` runs" |
| Prose in `README.md`, `AGENTS.md`, `docs/language.md`, `docs/diagnostics.md`, `docs/pull-requests.md`, `rfcs/README.md` | name, extension, binary paths, `KarlHarnois/rslisp` links | knave, `.knv`, `bin/knave`, `knave-lang/knave` links |
| CLI help snapshot `cli__help_lists_every_command.snap` | `Usage: rslisp` | `Usage: knave` |

**Rust strings that restate the name although `Language` holds it.** `AGENTS.md` already forbids these, so the build makes them read `Language::NAME`:

- `BUILD_DIRECTORY = "target/rslisp"` in `src/cli/terminal.rs`, which decides where `build` and `run` write binaries, and the 10 `target/rslisp/...` paths that `tests/cli.rs` expects
- the `rslisp-run-` prefix in `tests/support/mod.rs` and the `rslisp-cli-` prefix in `tests/cli.rs`

**Places that hold the extension:**

| Place | Count |
| --- | --- |
| `.rslisp` source files | 2,014: 21 in `prelude/`, 1,987 in `tests/`, 3 in `examples/`, 3 in `xtask/fixtures/` |
| Snapshot files whose name holds `.rslisp` | 1,789 of 1,842 |
| Snapshot contents that hold `.rslisp` | 3,009 lines in 1,805 snapshots: 1,789 `input_file:` lines, 1,171 diagnostic `-->` locations and 49 other locations, such as `:::` notes and test report lines |
| `.rslisp` in other tracked files | 524 mentions in 36 files, 448 of them in `tests/cli.rs` and 20 `include_str!` paths in `expand/src/modules/sources.rs` |

The run fixture sidecars, `<fixture>.arguments` and `<fixture>.stdin`, drop the extension from their names, so none of the 14 moves. The prelude's module paths, such as `prelude/text`, hold no extension and no name, so none changes. The project file of [RFC 0061](0061-project-file.md) is `project.rslisp` there and has no code yet, so its build creates `project.knv`.

#### Option 1A: rename every place

Every row above changes, including the crate names and the dev shell variables that no user of the binary sees.

#### Option 1B: rename only what a user sees

The binary, the extension, the help and the docs change. The crate names, the Rust paths and the dev shell variables keep `rslisp`.

### Question 2: the Cachix cache

#### Option 2A: a new `knave` cache

A new cache needs a new Cachix cache with its own signing key, a new `CACHIX_AUTH_TOKEN` secret in the repository, `CACHIX_CACHE: knave` in `.github/workflows/ci.yml`, `new-tests.yml` and `release.yml`, the new URL in `docs/pull-requests.md`, and the new substituter and public key in the owner's `/etc/nix/nix.conf` and in `modules/core.nix` of the owner's nixos-config. Until CI fills it, the new cache holds no build of the merge base, so probing a pull request downloads nothing for older commits.

#### Option 2B: keep `rslisp.cachix.org`

Nothing changes. The cache name is the only place the old name stays, and no user of the binary sees it.

### Question 3: accepted RFCs

21 RFC files hold 242 mentions of the name, with links to `KarlHarnois/rslisp`, which GitHub redirects to `knave-lang/knave`.

#### Option 3A: leave accepted RFCs as history, rewrite live text

Accepted RFCs keep `rslisp`. `rfcs/README.md`, `rfcs/template.md`, every RFC still `Proposed` when the build lands, `README.md`, `AGENTS.md` and `docs/` change. `docs/language.md` gains one sentence: "Before [RFC 0062](../rfcs/0062-rename-to-knave.md), the language was named `rslisp`, with the `.rslisp` extension." A reader of an old RFC then finds the old name from the current rules.

#### Option 3B: rewrite every RFC

Every RFC says knave. `rfcs/README.md` forbids editing an accepted RFC's reasoning after it merges, and the accepted RFCs record what the owner decided under the old name, such as RFC 0058's counts of `.rslisp` files.

### Question 4: the build order

#### Option 4A: one pull request

One commit renames the name, the extension, the files and the snapshots. Every open pull request that touches a fixture, a snapshot, the prelude or `tests/cli.rs` conflicts with it at once, and review has to tell 1,789 renamed snapshots apart from the 25 files whose text changes by hand.

#### Option 4B: the name first, then the extension by script in a quiet window

RFC 0058 renamed the flat `let` this way.

1. **Stage 1, the name.** An ordinary pull request changes `Language::NAME` to `"knave"` and every name row of [Question 1](#question-1-every-place-the-name-appears). `Language::EXTENSION` stays `"rslisp"`, so no source file moves and each commit builds. The stage touches 25 files: 23 that are neither sources nor snapshots, with 144 mentions of the name, `prelude/test.rslisp` and the CLI help snapshot. Open pull requests that touch `Cargo.toml`, `flake.nix`, the docs or the `use rslisp::` lines conflict, and each conflict is a word.
2. **Stage 2, the extension.** In a window with no open pull request that touches `prelude/`, `examples/`, `tests/` or `xtask/fixtures/`, a script changes `Language::EXTENSION` to `"knv"`, runs `git mv` on the 2,014 source files and the 1,789 snapshot files, rewrites `.rslisp` to `.knv` in the 3,009 snapshot lines and the 524 mentions in other files, then runs the stage fixtures. It is one commit, because the extension and the files must change together for the commit to build. The script also checks that every changed snapshot line differs from its old line only by the extension, which is how the build reads 1,805 snapshots it did not write by hand.

The instruction counts that CI posts compare each program of the corpus by name. If the comparison keys programs by file name with the extension, stage 2's comparison shows each program as removed and added. The build checks this before stage 2 and keys the comparison on the file stem if needed, in a commit of its own.

#### Option 4C: the extension first

The script runs before the name changes. The window opens earlier, while the code that names the language still says `rslisp`, and the stage that conflicts with the most pull requests runs before the one that conflicts with few.

### Question 5: changes outside the repository

None of these is in the repository, so the owner makes each one.

1. **Ringleader:** `~/.config/ringleader/config.toml` names the repository `rslisp` at `/home/karl/Documents/hobby/rslisp`, and worktrees live under `~/.local/share/ringleader/worktrees/rslisp/`. The owner renames the repository entry to `knave`, moves the main checkout to `/home/karl/Documents/hobby/knave` after stage 2 merges, when no session runs, and lets new worktrees take the new path.
2. **Local state:** the old prompt history at `~/.local/state/rslisp/history` and the slot directory at `~/.cache/rslisp/build-slots` stop being read. The owner moves the history to `~/.local/state/knave/history` to keep it, and deletes the slot directory.
3. **Shell environment:** a `RSLISP_BUILD_SLOTS` set in the owner's shell becomes `KNAVE_BUILD_SLOTS`.
4. **nixos-config:** it has no flake input for the repository. Its only mention is the `rslisp.cachix.org` substituter and key in `modules/core.nix`, which stays under [Option 2B](#option-2b-keep-rslispcachixorg).
5. **agent-config:** it holds no mention of `rslisp`, so nothing changes.
6. **The GitHub repository:** it is already `knave-lang/knave`. The owner keeps the redirect from `KarlHarnois/rslisp` for the links in accepted RFCs.

## Recommendation

- **Question 1: Option 1A.** A crate named `rslisp` beside a binary named `knave` is a second name that every reader of `Cargo.toml`, a `use` line or a dev shell variable has to map, and "state each piece of knowledge once" covers names. The build also makes the three restated strings read `Language::NAME`, so the next rename, if one comes, touches fewer places.
- **Question 2: Option 2B.** The owner decided it, as [Decision](#decision) point 5 records.
- **Question 3: Option 3A.** It follows the rule in `rfcs/README.md`, keeps every current rule in knave, and the one sentence in `docs/language.md` lets a reader map the old name.
- **Question 4: Option 4B.** Stage 1 is small enough to review by hand and to merge while other work is open. Stage 2 is mechanical, so a script does it and checks it, and only it needs the quiet window. Option 4C holds the window open for the larger stage while the smaller one waits.
- **Question 5:** the six owner actions above, after stage 2 merges. The Ringleader rename and the checkout move also wait until no session runs.

## Rejected options and why

- **Option 1B, rename only what a user sees:** it leaves a second name in the crates, the Rust paths and the dev shell, which every contributor has to map to the first.
- **Option 2A, a new cache:** the owner kept `rslisp.cachix.org` and left the cache rename to later work.
- **Option 3B, rewrite every RFC:** `rfcs/README.md` forbids editing an accepted RFC's reasoning, and the old counts and decisions are true of the old name.
- **Option 4A, one pull request:** it conflicts with every open pull request that touches a fixture, and it mixes 25 files changed by hand with thousands of mechanical renames.
- **Option 4C, the extension first:** it needs the quiet window for the larger stage while the smaller stage, which can merge beside other work, waits.

## Rule

[docs/language.md: The knave language](../docs/language.md#the-knave-language)
