# RFC 0047: A first REPL

**Status:** Accepted. Its `debug` bound in rules 3 and 5, including rule 3's "A result prints when its type implements `debug`" and rule 5's `pr-str` bound, and the `note` and `value` rows of its `--json` table, are superseded by 0048. Rule 4's case of a module error, the `note` row of its `--json` table, rule 5's hold-back of as many bytes as the marker is long, and rule 5's inline `(println "<marker>" (pr-str form))` are superseded by 0049. Rule 6's rejection of a definition input when the buffer fails to check, with "so the buffer never holds a definition that does not check", and its rejection with `L0203` of a replacement that would drop a name, are superseded by 0051.
**Date:** 2026-11-03

## Problem

The language has no interactive prompt. To try one expression, a user writes a file with a `main`, runs `rslisp run`, and edits the file again for the next expression. An editor cannot ask the compiler for the value of the form under the cursor either. RFC 0041 names a future REPL that reuses the `doc` lookup, and the scoped `let` RFC (pull request 276) names a REPL that treats its prompt as a module's top level, but no RFC designs one.

A REPL for this language has three constraints that a REPL for an interpreted Lisp does not have:

- A program runs only after rustc compiles it, so every expression pays a rustc run.
- A module's top level holds only definitions. `(+ 1 2)` in a file is `L0207`, "expected a definition at module level".
- A value has a Rust type and an owner. The compiler has no runtime that can hold a value between two programs.

Every expression pays the whole pipeline and a rustc run, so latency decides whether a design is usable. The numbers below are wall-clock medians of 7 runs on `main` at `5501598`, on a 4-core Linux container, with rustc 1.95.0 from `nix develop` and the compiler built by `nix develop -c cargo build --release`. The session program has 38 definitions: 4 `defstruct`, 1 `enum`, 4 `def` and 29 `defn`. The prototype files live outside the repository and are not part of this RFC.

| Command | Median | Min | Max |
| --- | --- | --- | --- |
| `cargo run --release --quiet -- run examples/hello.rslisp` | 0.276 s | 0.238 s | 0.318 s |
| `rslisp run examples/hello.rslisp` | 0.203 s | 0.183 s | 0.213 s |
| `rslisp run` on `(prn (+ 1 2))` with no definitions | 0.211 s | 0.186 s | 0.247 s |
| `cargo run --release --quiet -- run` on the session, one expression | 0.323 s | 0.295 s | 0.339 s |
| `rslisp run` on the session, one expression | 0.272 s | 0.250 s | 0.310 s |
| `rslisp run --release` on the session, one expression | 0.321 s | 0.289 s | 0.352 s |
| `rslisp run` on the session, 25 expressions that reach every definition | 0.360 s | 0.346 s | 0.402 s |
| `rslisp run` on the session, the 25 expressions 4 times (100) | 0.376 s | 0.349 s | 0.413 s |
| `rslisp expand` on the session | 0.011 s | 0.011 s | 0.016 s |
| `rslisp check` on the session | 0.021 s | 0.020 s | 0.029 s |
| `rslisp emit` on the session, rustfmt included | 0.047 s | 0.040 s | 0.052 s |
| `rustc` on `fn main() {}` | 0.160 s | 0.141 s | 0.166 s |
| `rustc` on the emitted Rust of hello | 0.185 s | 0.157 s | 0.197 s |
| `rustc` on the emitted Rust of the session, one expression | 0.224 s | 0.203 s | 0.241 s |
| `rustc -C opt-level=3` on the same Rust | 0.248 s | 0.230 s | 0.265 s |

A second run of four rows on `main` at `a4df911b`, on an 18-core Linux machine, gives medians of 0.139 s for `rslisp run examples/hello.rslisp`, 0.139 s for `rslisp run` on `(prn (+ 1 2))`, 0.005 s for `rslisp check examples/hello.rslisp`, and 0.099 s for `rustc` on `fn main() {}`. The machine is faster, and rustc still takes most of a run.

Four facts follow:

- **An expression costs 0.2 to 0.4 seconds.** rustc takes about 80% of it, and 0.16 seconds of that is the fixed cost of compiling and linking any program.
- **The program's size barely matters.** Emission keeps only the definitions that `main` reaches, so the Rust of an expression that uses 4 of the 38 definitions is 50 lines long, and rustc sees nothing else.
- **Checking definitions needs no rustc run.** `rslisp check` accepts a module with no `main`, so the front end alone checks the 38 definitions in about 20 ms.
- **One program of many expressions costs about as much as one.** 100 expressions compile and run in 0.376 s, against 0.272 s for one.

## Options

Each option shows one session at a prompt. A line that starts with `> ` is what the user types, and the other lines are what the REPL prints:

```text
> (defn square (n) (* n n))
> (defstruct point :x i64 :y i64)
> (->point (square 2) 3)
Point { x: 4, y: 3 }
> (println "side " (square 3))
side 9
```

### A: evaluate forms against a module's text

The compiler gets one new command, `rslisp eval`, and nothing in it persists between two runs. It takes a module's text and one or more forms, checks the text as one module, runs the forms once in that module's scope, and prints each result. The text is the session. An editor sends its buffer, and the prompt keeps a buffer of its own.

```sh
rslisp eval src/geometry.rslisp '(area 2 3)'
rslisp eval --stdin src/geometry.rslisp '(area 2 3)' < unsaved-buffer
```

For the third input of the session, the prompt's buffer holds the first two inputs, and `eval` compiles that text with a generated `main`:

```lisp
(defn square (n) (* n n))
(defstruct point :x i64 :y i64)
(defn main ()
  (prn (->point (square 2) 3))
  (do))
```

```rust
fn square(n: i64) -> i64 {
    n * n
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Point {
    x: i64,
    y: i64,
}

fn make_point(x: i64, y: i64) -> Point {
    Point { x, y }
}

fn main() {
    crate::rt::restore_default_sigpipe();
    println!("{:?}", make_point(square(2), 3));
}
```

For the fourth input, the result is `unit`, so `eval` does not wrap it in `prn`:

```lisp
(defn square (n) (* n n))
(defstruct point :x i64 :y i64)
(defn main ()
  (println "side " (square 3))
  (do))
```

```rust
fn square(n: i64) -> i64 {
    n * n
}

fn main() {
    crate::rt::restore_default_sigpipe();
    println!("side {}", square(3));
}
```

The third input's struct and constructor are gone from this Rust, because nothing in this `main` reaches them. Each rule of option A follows.

#### 1. `rslisp eval` checks the text, then runs the forms once

`rslisp eval <file> <forms>` reads the module's text from `<file>`. With `--stdin`, it reads the text from standard input instead, and `<file>` still names the module, so an editor can send a buffer with unsaved edits. `<forms>` is one argument that holds one or more forms.

`eval` checks the text as one module, as `rslisp check` does, so a module with no `main` checks. It resolves `(use shapes circle)` from a root directory, which `--root <directory>` names and which defaults to the directory of `<file>`, as `check` resolves from its entry file. An editor passes the directory of the program's entry file, so every module of the program resolves as it does under `run`. Once the language has a project file ([#294](https://github.com/KarlHarnois/rslisp/issues/294)), `--root` defaults to the directory of the nearest one above `<file>`, and an explicit `--root` keeps working.

`eval` then lowers the forms as the body of a generated `main` in the module's scope, as `rslisp doc --file` lowers its lookup today through `ExpansionSession::expression`. A form sees every definition of the module and the prelude. The forms run once, in order, in one program, so one call pays one rustc run however many forms it holds. A form whose result is not `unit` prints its result by rule 3. The generated `main` ends with `(do)`, so every form, the last one included, is a statement and `main` returns `unit`. A closure or an iterator as the last form then gets rule 3's note, not `L0455` about a `main` the user never wrote, nor `L0510`.

The forms are the forms of one body, so every rule of a body holds, and `eval` adds no rule for a binding. A flat binding binds its name for the forms after it, and a body that ends with one has the value `unit`, so it prints nothing:

```lisp
(defn main ()
  (let total (+ 2 3))
  (do))
```

```rust
fn main() {
    let total: i64 = 2_i64 + 3_i64;
}
```

`rslisp eval` with the forms `(let x 5) (* x 2)` prints `10`, as `(defn main () (let x 5) (prn (* x 2)))` does. A definition is not a body form, so `(defn f () 1)` as a form is `L0208`, "`def` can only appear at module level", as it is in any body. The scoped `let` RFC (pull request 276) changes which form binds flat. `eval` follows whatever a body does, so that change cannot break this design.

Nothing persists between two runs. Each run is a new process, so a `def` whose value is not constant, which the emitter writes as a `std::sync::LazyLock`, is computed once in each run that uses it, as it is once in each `rslisp run`:

```lisp
(defn noisy () (println "computing") 42)
(def answer (noisy))
```

```rust
fn noisy() -> i64 {
    println!("computing");
    42
}

static ANSWER: std::sync::LazyLock<i64> = std::sync::LazyLock::new(|| noisy());
```

The program reads `eval`'s standard input, as a program under `run` does. With `--stdin`, the text has used it up, so `(read-line)` returns `Ok(None)` at once. `eval` exits 1 when the text or the forms have errors, 2 on a usage error, and otherwise with the program's exit code, as `run` does. A program stopped by signal N gives `128 + N`, as a shell reports it, where `run` exits 1 today, or 0 when a closed output stops it. `eval` keeps `run`'s 0 for a closed output, so `rslisp eval f '(spin)' | head -1` succeeds as `run` does.

#### 2. The generated `main` is the entry

Today both the type checker and the emitter find the entry by the name `main`: the checker picks its reachability roots and reports `L0455` by that name, and the emitter places the start-up call by matching `fn main() {`. `eval` gives its own `main` a binding that no source name can refer to, as `defstruct` does for its positional constructor, and the checker and the emitter take the entry from that binding. A module's own `main` stays in the text, is checked as `rslisp check` checks it, and does not run.

The emitter gives a written name its Rust name first and renames only a name the compiler introduced, with a `_2` suffix, so a module's `main` would keep `fn main` and the entry would become `main_2`. The emitter reserves the Rust name `main` for the entry binding and renames a written `main` instead, so rustc runs the entry.

#### 3. Printing the result

A result prints when its type implements `debug`, with the Rust that `prn` emits, as Clojure's REPL prints with `pr`. A `unit` result prints nothing, so `(println "hi")` prints `hi` once and nothing else.

Only the type checker knows a form's type, so `eval` asks it before it writes `main`:

1. A new check-stage query infers the type of each form in its place in the body and returns it as a value. It reports problems as `Diagnostic` values and prints nothing.
2. If the type is `unit`, the form stays as written.
3. Otherwise `eval` asks the checker whether the form can be the argument of `prn`. It reads `prn` from the prelude's bindings, so the bound that decides is the one `prn` states, `(where ((a debug)) (fn ((ref a)) unit))`, and the compiler never names `debug` in Rust.
4. If `prn` accepts it, the form becomes `(prn form)`, with `prn` resolved in the prelude's scope, so a module's own `prn` does not replace it.
5. If `prn` does not accept it, as for a closure or an `iterator`, the form stays as written and `eval` prints a note with the inferred type, such as ``note: the value has type `(iterator i64)`, which does not implement `debug` ``. A user who sees no output and no note would not know that the form ran.
6. If the type is still open after inference, as for `none` or `(vec)`, `eval` reports `L0407` with its label on the form and help that shows the form inside `the`, such as `(the (option i64) none)`.

Today `L0407` labels the generated call and gives help about bindings:

```text
error[L0407]: type annotations needed
 --> none.rslisp:1:16
  |
1 | (defn main () (prn none))
  |                ^^^ cannot infer the type of this value
  |
  = help: annotate the binding, as in `(let (name i64) value)`, or add a `type` signature
```

The wording for a form of `eval` lives in `TypeProblems`, beside today's.

`prn` prints Rust's `Debug` form, so a struct prints as `Point { x: 4, y: 3 }` and a string prints with quotes, as `"hello, ana"`. Printing a value in the language's own syntax is a separate change to `prn`, which comes before the REPL and which the REPL follows. This RFC does not design it.

#### 4. A source per form, at its place in the text

The forms are their own source in the `SourceMap`, named `<eval>` as `rslisp doc` names its term `<term>`. An editor evaluates a form that sits in its buffer, so `--line <n> --column <n>` give the position where the forms start in the module's text. Columns count UTF-8 bytes, as LSP 3.17 counts them with the `utf-8` position encoding, and lines and columns count from 1, as the compiler's diagnostics do, where LSP counts from 0. The source of the forms then takes the module's name and starts at that position, so a diagnostic on a form points at the form's own line in the buffer, as a diagnostic on a definition does. The `SourceMap` needs a source that starts at a position of another source's text.

With `--line` and `--column`, the forms MUST be the module's text at that position, or `eval` exits 2. The forms then sit inside the module that `eval` checks, so where each form sits decides what happens to it. These rules apply to each form on its own, and a definition inside a top-level `do` counts as top level:

- A form that is a definition at the module's top level is already part of the module. `eval` checks the module, replies with a `note` that the module defines it, and runs nothing.
- Any other form at the module's top level is left out of the module check, so it is checked once, as a form of the generated `main`, and not reported as `L0207` beside its result.
- A form inside a definition stays in that definition for the module check, and runs again as a form of the generated `main`. It sees only the module's definitions and the prelude, not that definition's parameters or locals. If the module check reports a diagnostic whose span lies inside the forms, `eval` reports the module's diagnostics only and does not check or run the forms, so each problem shows once.

A rustc error maps back through the spans of the emitted Rust, as `RustcDiagnosticMapper` does for `rslisp run`, so it points at the module's text or a form too. An emission error, such as `L0602` when `admin?` and `is-admin` both become `is_admin` in Rust, shows when a form reaches both definitions, because emission keeps only what `main` reaches and `check` does not emit:

```text
error[L0602]: `admin?` and `is-admin` both become `is_admin` in Rust
 --> clash.rslisp:2:7
  |
1 | (defn admin? () true)
  |       ------ this also becomes `is_admin`
2 | (defn is-admin () false)
  |       ^^^^^^^^ this becomes `is_admin`
  |
  = help: rename one of them
```

#### 5. `--json` keeps the value apart from the output

`rslisp eval --json` writes one JSON object per line, so an editor can tell the program's output, the values and the diagnostics apart. Each object carries one field, and `id` when the call gave `--id <text>`:

| Field | Carries |
| --- | --- |
| `out`, `err` | The program's standard output and standard error, as they stream, under nREPL's names |
| `value` | One form's result, as `prn` prints it, under nREPL's name, with `form`, the number of the form from 1 |
| `diagnostics` | Diagnostics, each with its `severity`, `code`, `message`, `file`, `line`, `column`, `end_line` and `end_column` in that file's text, in the units of `--line` and `--column` with the end one past the span, and the `rendered` text, which keeps the compiler's character columns |
| `note` | A line `eval` prints itself: the type of a value without `debug`, or that the module defines the form |
| `status` | The last object, `{"status": "done", "exit": <code>}`, with the code `eval` exits with. A program stopped by signal N gives `128 + N`, as a shell reports it, except that a closed output gives 0, as under `run`. A usage error writes no JSON: it prints to standard error and exits 2 |

```sh
rslisp eval --json --id 7 --stdin --line 42 --column 3 src/geometry.rslisp '(area 2 3)' < buffer
```

```json
{"id": "7", "out": "computing\n"}
{"id": "7", "value": "6", "form": 1}
{"id": "7", "status": "done", "exit": 0}
```

For `(area 2 "3")` at the same position, sent with `--id 8`, the form sits in a definition, so the module check finds the error on `"3"`, inside the form. `eval` reports it once, from the module check, and runs nothing:

```json
{"id": "8", "diagnostics": [{"severity": "error", "code": "L0401", "message": "mismatched types", "file": "src/geometry.rslisp", "line": 42, "column": 11, "end_line": 42, "end_column": 14, "rendered": "error[L0401]: mismatched types ..."}]}
{"id": "8", "status": "done", "exit": 1}
```

The program's output and its values both go to its standard output. In `--json` mode, a form that rule 3 prints becomes one statement that prints a marker and then the value, `(println "<marker>" (pr-str form))`, with `println` and `pr-str` resolved in the prelude's scope. Rust evaluates the arguments before it prints, so the form's own output comes before the marker, and the form's temporaries live until the value is printed. A shape that binds the value first would fail on a value that borrows a temporary, such as `(trim (as-str (str " a" "b ")))`, which rustc rejects as `E0716` in a `let`. On `main`, the statement emits:

```rust
println!("MARK{:?}", " ab ".to_string().as_str().trim())
```

and `(println "MARK" (pr-str (do (print "x") 6)))` prints `xMARK6`. The marker is a random string that `eval` makes for each run, so no program prints it by chance. `eval` streams standard output as `out` up to a marker and sends the rest of that line as `value`, so for `(do (print "x") 6)` the editor gets `out` `x` and `value` `6`. It holds back as many bytes as the marker is long, so a marker split across two reads is still found. The forms run in order, so the first marker belongs to the first form that prints a value, and `eval` numbers each `value` by its form.

A form with a `unit` result or a type without `debug` gets no marker and no `value`, because `pr-str` has the same bound as `prn`, `(where ((a debug)) (fn ((ref a)) string))`. A type without `debug` gets rule 3's `note`.

Each call is one process, so the editor stops a call by stopping its process. The shapes follow the leading REPL protocols: nREPL, SWANK, Jupyter and LSP each tag a reply with its request's id and end a request with a done status, nREPL names the streams `out`, `err` and `value`, and LSP places a diagnostic by line and column. A stateless `eval` needs neither the id nor the done status, but with them a client written today stays unchanged if the engine later becomes a long-lived server (#290, or option C in #292).

#### 6. `rslisp repl` is a thin prompt over `eval`

The prompt's only state is a buffer of the definitions entered so far, as text. It reads one input at a time, a complete form, and asks the expander whether the input is a definition: the expander already expands each top-level form of a file until its head is a kernel definition form, and reports `L0207` when it is not. An input that a file would accept at its top level is a definition, and any other input is an expression. A line that holds several forms is that many inputs, in order.

A definition input replaces, as text, every earlier input that defines or imports a name it defines or imports. If a replaced input defines or imports a name that the new input does not, as a `defstruct` replaced by a `defn` of its constructor's name or a `do` of two functions replaced by one of them, the prompt rejects the input and reports `L0203` for the repeated name, with the earlier input as its first definition, so no definition disappears unseen. Otherwise the input joins the buffer. The prompt then checks the buffer as `rslisp check` checks a file. If the check fails, the prompt prints the diagnostics, rejects the input, and keeps the buffer as it was, so the buffer never holds a definition that does not check. The definitions that use a replaced one use the new one, because each expression compiles the whole buffer again:

```text
> (defn square (n) (* n n))
> (defn cube (n) (* n (square n)))
> (cube 3)
27
> (defn square (n) (+ n n))
> (cube 3)
18
```

An expression input runs through `eval` against the buffer, as one form. A flat binding at the prompt is an expression, so it binds its name for its own body and prints nothing, as it does at the end of a function body. `rslisp repl` takes `--root <directory>`, which defaults to the current directory, and resolves `use` from it as `eval` does. A `use` input is a definition input, so it replaces by the same rule: `(use other circle)` replaces an earlier `(use shapes circle)` or `(defn circle ...)`, and `(use shapes circle)` after `(use shapes circle square)` is rejected, because it would drop `square`. Each input is its own source, named `<input N>`, so a diagnostic points at the text the user typed. The expander takes a module from one source today, so it gains an entry that takes the buffer's inputs.

The prompt differs from a file in three ways:

- An input that is not a definition runs once, as the body of a generated `main`, and its result prints by rule 3.
- A definition replaces the earlier definition of the same name, where a file reports `L0203`, unless that would drop a name.
- Each definition is checked when it is entered, so it must come after the definitions it uses. Mutually recursive functions go in one input, a `do` of both, as `(do (defn even? ...) (defn odd? ...))`.

#### A stateful session, rejected

The first draft of option A kept a session instead of a text. Each definition input joined a session module, an editor kept one session module per file through `load` and `eval` requests over a long-running `rslisp repl --json`, and a flat binding that was a whole input showed its value with a new warning. The owner rejected it for these reasons:

- Every expression compiles the whole program again, so a stateful session saves almost nothing, and it costs many rules: replacement by keys in each namespace, reconciling `load` and `eval` across modules, rounds that leave out forms that no longer check, and notes for inputs that cannot come back.
- The editor needs only stateless evaluation against its buffer, and the buffer is already the truth.
- The flat-binding warning was an exception for convenience, and the code it took, `L0246`, belongs to the `rust` clauses on `main`.
- The draft bundled at least two designs: the REPL and the editor protocol.

### B: replay every input on each run

gore (Go), icr (Crystal) and INim (Nim) keep every input, expressions included, and run all of them again for each new input. For the fourth input of the session, `main` replays the third input as a statement before it runs the fourth:

```lisp
(defn square (n) (* n n))
(defstruct point :x i64 :y i64)
(defn main ()
  (->point (square 2) 3)
  (println "side " (square 3)))
```

```rust
fn square(n: i64) -> i64 {
    n * n
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Point {
    x: i64,
    y: i64,
}

fn make_point(x: i64, y: i64) -> Point {
    Point { x, y }
}

fn main() {
    crate::rt::restore_default_sigpipe();
    make_point(square(2), 3);
    println!("side {}", square(3))
}
```

The fifth input then runs the `println` again and prints `side 9` a second time.

A local binding can then last across inputs, because every input is a line of the same `main`. The costs:

- Every side effect repeats. `(println "saved")` prints again after every later input, and an input that writes a file writes it again.
- A value that depends on time or on random numbers changes at every input, so a value the user saw before is not the value that a later input uses.
- An input that panics stops every later run, so the REPL must remove it after it fails.
- Each input runs every earlier input, so it gets slower as the session grows. The table shows that a program of 100 replayed expressions compiles and runs in 0.376 s, close to one expression, but a replayed expression that computes for a second adds that second to every later input.
- A REPL that hides the output of replayed inputs must still run them, and hiding output does not undo a side effect such as a written file.

### C: keep values alive across inputs

evcxr (Rust) compiles each input as a dynamic library that the REPL process loads, and stores each variable in a type-erased slot that later inputs read back. Scala's REPL and JShell compile each input into a generated wrapper object or class, and a later input refers to the earlier wrapper's fields. A value is computed once and lives until the REPL quits.

A sketch of what this language would emit for two inputs, written by hand:

```lisp
(def total (+ 2 3))
(prn total)
```

```rust
#[unsafe(no_mangle)]
pub fn input_1(slots: &mut Slots) {
    let total: i64 = 2 + 3;
    slots.store("total", Box::new(total));
}

#[unsafe(no_mangle)]
pub fn input_2(slots: &mut Slots) {
    let total: &i64 = slots.load("total");
    println!("{:?}", total);
}
```

The costs:

- Much more design and code: a runtime crate that holds the slots, a dynamic library per input, a loader, and a shared crate for the types that the inputs define, because Rust has no stable ABI across separately compiled libraries.
- Ownership is checked over one whole program today. A value that moves in one input and is used in a later one needs rules for ownership across inputs, which the language does not have.
- A type redefined while a value of the old type is alive needs its own rule.

Option A does not block C. A's printing rule, generated entry and diagnostics apply to C unchanged. C changes only how a program holds earlier values, so it is a possible later step if a `def` that is computed again in each run turns out to cost users too much.

### D: an interpreter

An interpreter would evaluate each input directly, with no rustc run. `expand/src/interpret/` already evaluates code, but only macro code at compile time. It runs the kernel forms and the `code` module on forms, and it cannot run a `rust` template, so it cannot run `+`. It has no runtime meaning for Rust types, ownership, traits or generics. An interpreter for programs would restate in Rust every `rust` template the prelude states, which "NEVER restate in Rust a name or a fact that the prelude states" forbids, and its results could differ from the compiled program's.

`+` expands to `add`, which the prelude states as a `rust` template, so only the emitted Rust runs it:

```lisp
(def add (rust "{0} + {1}" (where ((n number)) (fn (n n) n))))
(let total (+ 2 3))
```

```rust
let total: i64 = 2_i64 + 3_i64;
```

The REPL's purpose is to show what a compiled program does, so D is not an option for this language.

## Recommendation

Option A, with the six rules above.

- It reuses the existing pipeline: reading, expansion, checking, emission and rustc stay as they are. The new compiler code:
  - In the expander: lowering forms as the body of a generated `main` in a module's scope, the entry binding that no source name can refer to, and a module entry that takes the prompt's inputs as several sources.
  - In the checker: the type query of rule 3, the entry taken from the entry binding, and the `L0407` wording for a form of `eval`.
  - In the emitter: the same entry binding, and the Rust name `main` reserved for it, so a written `main` is renamed.
  - In the sources: a source that starts at a position of another source's text.

  `rslisp eval`, its `--json` mode and the prompt's buffer are CLI code. No new diagnostic code is needed.
- A form runs once, so the user sees each side effect once.
- An evaluation costs 0.2 to 0.4 seconds, and a definition at the prompt about 20 ms, on the machine above.
- Nothing persists between two runs, so the editor and the prompt share one rule: the text is the session. The prompt differs from a file in three ways, which rule 6 lists.
- It leaves C open.

A has costs:

- A `def` whose value is computed is computed again in each run that uses it, so its side effects repeat and a time or a random value changes.
- A local binding never lasts beyond one evaluation.
- An emission error or an error that only rustc finds in a definition shows on its first use.

The settled details:

- **Command names:** `rslisp eval` and `rslisp repl`, subcommands like `run`. Both take `--release`, as `run` does.
- **Build profile:** debug by default. rustc takes 0.224 s in debug and 0.248 s with `-C opt-level=3` on the session above, and debug keeps overflow checks. `--release` exists for a form that computes for long.
- **Build files:** a new directory under the system's temporary directory for each `eval` and for each prompt process, removed when it ends. An `eval` started in a home directory must not create a `target/` there.
- **Multi-line input:** the prompt keeps reading lines while the reader reports an unclosed list (`L0101`) or an unclosed string (`L0103`) at the end of the text. It asks the reader, so a `(` inside a string or a comment does not count. Any other reader error, such as an extra `)` (`L0102`), ends the input at once. The prompt is `> `, and continuation lines show two spaces, so a pasted block lines up. Prompts show only when standard input is a terminal.
- **Quitting:** end of input, which Ctrl-D sends on a terminal, quits the prompt with exit code 0, also after inputs that failed. An unclosed list at end of input is `L0101` and an unclosed string is `L0103`, then the prompt quits. No command is needed to quit, and no character or keyword is added. The prompt exits 2 when it cannot start, and it looks for rustc at start-up, because a definition input never runs rustc.
- **Ctrl-C:** while rustc or the program runs, Ctrl-C stops that process and the prompt keeps its buffer.
- **Panics:** a panic stops only the program. The prompt shows the panic message, as `run` shows it, and keeps its buffer. Mapping a panic's location from the generated Rust to the source is a separate change, which `run` needs too.
- **`(doc term)`:** an ordinary form. It expands to the page as a string literal and prints through `prn`, so the REPL reuses RFC 0041's lookup with no code of its own:

  ```text
  > (defn square (n) "Multiplies a number by itself." (* n n))
  > (doc square)
  "(defn square (n))\n\nMultiplies a number by itself."
  ```

- **Tests:** tests in `tests/cli.rs` run `rslisp eval`, with and without `--json`, and `rslisp repl` with standard input from a fixture of fake inputs, and record standard output, standard error and the exit code.

These steps come later, each with its own design:

- [#290](https://github.com/KarlHarnois/rslisp/issues/290): evaluating from Neovim through `rslisp eval --json`.
- [#291](https://github.com/KarlHarnois/rslisp/issues/291): line editing and history at the prompt.
- [#292](https://github.com/KarlHarnois/rslisp/issues/292): keeping values alive across inputs, option C.
- [#293](https://github.com/KarlHarnois/rslisp/issues/293): forwarding standard input from the editor to the program.
- [#294](https://github.com/KarlHarnois/rslisp/issues/294): a project file, so `eval` and the prompt find the project root.

## Decision

The project owner chose option A on 2026-11-02.

On 2026-11-03 the owner narrowed option A to the stateless `rslisp eval` with its `--json` mode, and to the thin `rslisp repl` prompt whose only state is a buffer of definitions, as rules 1 to 6 describe. The stateful session of the first draft is rejected, for the reasons under option A. Evaluating from Neovim (#290), line editing and history (#291), keeping values alive (#292), forwarding standard input from the editor (#293) and a project file that sets the root (#294) are later steps, each designed in its own issue.

The owner also decided on 2026-11-03 that `--json` takes the message shapes of nREPL and LSP: positions as line and column, a request id on every object, a final done status, and nREPL's names for the streams and the value. The engine stays stateless, and the shapes keep the editor client unchanged if it later becomes a server.

## Rejected options and why

- **A stateful session (a variant of A):** every expression compiles the whole program again, so a session saves almost nothing and costs many rules, the editor needs only stateless evaluation against its buffer, and its flat-binding warning was an exception for convenience.
- **B: replay every input:** every side effect repeats, a time or random value changes at each input, and every input pays for all earlier ones. A local binding that lasts across inputs does not outweigh output that the user did not ask for.
- **C: keep values alive:** not rejected, deferred to #292. It needs a runtime, a dynamic library per input and ownership rules across inputs, and A does not block it.
- **D: an interpreter:** the interpreter runs macro code only, and running programs would restate every `rust` template of the prelude in Rust.

## Rule

The build that lands `rslisp eval` states its rule in [docs/language.md](../docs/language.md), in a new section on evaluation and the prompt, which states each rule of option A, including that nothing persists between two runs and the three ways the prompt differs from a file.
