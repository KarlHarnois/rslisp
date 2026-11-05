# RFC 0055: Tests written in the language

**Status:** Accepted
**Date:** 2026-11-05

## Problem

The language has no way to write a test. Every test of the prelude and of the compiler is a fixture under `tests/fixtures/`, whose snapshot records what a whole program prints. A fixture is the right test for a diagnostic or for the emitted Rust, but a poor one for a function's behavior: the expected value sits in a snapshot file away from the code, one fixture costs one rustc run, and nothing tells which prelude names no fixture exercises.

The project owner asked for three things:

- `deftest` and `assert` in the prelude, so prelude functions and user programs carry tests beside the code they test.
- An `rslisp test` command that runs them.
- A first report of static name coverage: the exported prelude names that no test references. The expander resolves every name, so the report needs no run.

Tests of compile errors stay fixtures.

The probes below ran on a release build of `a0c6a844`, built with `nix develop -c cargo build --release`, with rustc 1.95.0 on an 18-core Linux machine. `origin/main` at `303a3380` has the same compiler: since `a0c6a844` it adds only a version number and an RFC. Prototypes of `deftest`, `assert` and a runner, written as user modules over today's `code` module, live under `target/scratch/v2/` and `target/scratch/v3/` and are not part of this RFC. They have no `form-location` and no `program-modules`, so they print a placeholder location and run the tests of one module.

### What the probes found

- **The prelude can express all three forms today.** The prototype's `deftest` defines the test's function under an introduced name and notes it through a nested macro call. Its `run-tests` reads the notes with `module-notes` and expands to one runner call per test. Its `assert` turns the written call into a string literal with `string`. Its runner is a `rust` template over `catch_unwind`. A run of the first prototype, which printed its own lines:

  ```text
  running 4 tests
  test pads-left ... ok
  test trims-right ... FAIL
  FAIL trims-right: Removes trailing spaces, not inner ones. (loc)
      (assert (= (pad-left "a") "a "))
      left:  "  a"
      right: "a "
  test unwraps ... FAIL
  FAIL unwraps (loc)
  called `Option::unwrap()` on a `None` value
  test is-even ... FAIL
  FAIL is-even (loc)
      (assert (> (len "ab") 7))
  1 passed, 3 failed
  ```

- **An introduced test function stays out of the user's way.** With a user function `test-pads` beside a test `pads` whose function is the introduced `test-pads`, `rslisp signatures` prints only `(type test-pads (fn () unit))` and `(type main (fn () unit))`, and `emit` names the user's function `test_pads` and the test's `test_pads_2`.
- **A runner can write marked records.** The second prototype's runner writes one record per event, and an `assert` panics with its location and its detail as a pair, which the runner reads back with `downcast`. A test that prints `hello` with no newline puts the next record on the same line:

  ```text
  <m>(start 3)
  <m>(test "rec/equal")
  <m>(fail "rec.rslisp:3:3" "(assert (= 1 2))\n1\n2")
  <m>(test "rec/print")
  hello<m>(ok)
  <m>(test "rec/unwrap")
  <m>(fail "rec.rslisp:4:1" "called `Option::unwrap()` on a `None` value")
  ```

- **`emit` and `build` already drop a test.** The emitter writes only the functions that the program reaches from `main`. A `deftest` that nothing calls emits no Rust, so a program's emitted Rust is the same with and without its tests, and no flag such as Rust's `#[cfg(test)]` is needed.
- **`check` and `build` still check a test.** A test with a type error, `(assert (= (twice "a") 4))`, fails `rslisp build` with `L0401` at `"a"`, in the user's source. A broken test fails the build that it sits in.
- **A definer in a body is `L0208` and names `def`.** `(defn helper () (defn inner () 1) (inner))` reports ``error[L0208]: `def` can only appear at module level`` at `(defn inner () 1)`. A `deftest` in a body reports the same at the `deftest` call. A macro cannot tell a body from a module level before it expands, so `deftest` cannot report its own position.
- **A note added inside a function body comes too late.** A `deftest` that notes itself from inside its function's body notes during the second pass, and a `run-tests` above the tests in the same module then finds no test. A note that the `do` of the expansion adds during the first pass reaches every `run-tests` of the module.
- **A panic reports a line of the generated Rust.** `(unwrap (get v 5))` under `rslisp run` prints `panicked at target/rslisp/probe1/main.rs:10:46`. A macro cannot learn a form's line either: the `code` module has no built-in that returns a location.
- **A crash leaves its record only after a flush.** Rust's standard output is line-buffered. With a flush after the record of a test's start, the record shows before `(exit 0)`, `(exit 3)`, an abort and a stack overflow. Run directly, the aborting test program ends with status 134, signal 6.
- **Name coverage works in macro code.** A macro that walks the stored test bodies, collects their symbols, and keeps each candidate name that no symbol refers to by `same-binding?` expands to `"untested: (prn len)"` over three tests and the candidates `twice shout str prn len`.
- **The prelude exports 221 distinct names** in the `export` lists of `prelude/prelude.rslisp`, kernel forms included.

Build time of one program, measured with `time rslisp build`:

| Program | Wall time |
| --- | ---: |
| The first prototype, 3 tests | 0.29 s |
| `tests/fixtures/corpus/warehouse.rslisp`, 617 lines over its modules | 0.52 s |

Nearly all of it is rustc: `rustc` alone on the prototype's emitted Rust takes 0.27 s.

## Options

The design has eight independent choices. Each lists its options, and the recommended option comes first.

### Choice 1: what `deftest` defines and how a test is named

#### 1A: an introduced function, noted by a nested macro call

```lisp
(deftest pads-left
  (assert (= (pad-left "7" 3) "  7")))

(deftest trims-right
  "Removes trailing spaces, not inner ones."
  (assert (= (trim-right "a b  ") "a b ")))
```

```rust
fn test_pads_left() {
    crate::prelude::testing::check_equal(
        "text/format.rslisp:4:3",
        "(assert (= (pad-left \"7\" 3) \"  7\"))",
        &pad_left("7", 3),
        "  7",
    )
}
```

`deftest` expands to a `do` that holds the test's function and a nested macro call:

```lisp
(do
  (defn test-pads-left () (assert (= (pad-left "7" 3) "  7")))
  (note-test test-pads-left pads-left "" "text/format.rslisp:3:1" ((assert (= (pad-left "7" 3) "  7")))))
```

`deftest` builds the function's name from the written name, with the scopes of its own module, so the name is introduced, as the positional function of `car` is. Hygiene keeps it apart from every name the user writes. `rslisp check` and `rslisp signatures` leave it out, as Names in Rust states for an introduced function that no macro names, and its Rust name yields to an item the program wrote, so beside a user function `test-pads-left` the test emits as `test_pads_left_2`.

The nested `note-test` call carries the introduced symbol with its scopes. It adds one note under `tests` to its module's notes: the function's symbol, the written name, the first line of the docstring, the location of the `deftest` form, and the written body for coverage. The `do` adds the note during the first pass of the module body, so every `run-tests` finds every test of the module. `run-tests` calls the noted symbol and prints the written name.

`note-test` reads the module's `tests` notes first. When an earlier note holds the same written name, it fails at the second `deftest` under a new macro-stage code, `L0333` `duplicate-test`:

```text
error[L0333]: test `pads` is defined more than once
  --> text/format.rslisp:36:10
   |
33 | (deftest pads (println "test pads ran"))
   |          ---- first defined here
...
36 | (deftest pads (println "again"))
   |          ^^^^ defined again here
   |
   = help: rename one of the tests
```

The prototype gives this report under `L0301`, the code a `fail` takes without one.

A docstring is optional, and follows the rule of `defn`: a string that is the first body form and that another form follows. It goes only into the note. A test name is not a binding, so `(doc pads-left)` reaches only a user binding of that name, or is `L0201`.

A test's full name is its module's path and its written name: `text/format/pads-left`. The report prints full names. A passing test prints one line. A failing test ends its line with `FAIL`, then prints a line with its full name, the first line of its docstring after a colon when it has one, and the location of the failed `assert`, then the written `assert` call and, for `=`, both sides. The run starts with the number of tests and ends with a count line:

```text
running 2 tests
test text/format/pads-left ... ok
test text/format/trims-right ... FAIL
FAIL text/format/trims-right: Removes trailing spaces, not inner ones. (text/format.rslisp:20:3)
    (assert (= (trim-right "a b  ") "a b "))
    left:  "a b"
    right: "a b "
1 passed, 1 failed
```

`rslisp test` prints all of these lines from the runner's records, as 6A states.

#### 1B: a plain function under the written name

```lisp
(deftest pads-left (assert (= (pad-left "7" 3) "  7")))
```

```rust
fn pads_left() {
    crate::prelude::testing::check_equal("text/format.rslisp:1:20", "(assert (= (pad-left \"7\" 3) \"  7\"))", &pad_left("7", 3), "  7")
}
```

`deftest` expands to `(defn pads-left () ...)`. A test named after the function it tests redefines that function, `L0203`, so every test name needs a suffix or a prefix by convention, as Go's `TestXxx` does.

#### 1C: a string name, as Zig and ExUnit do

```lisp
(deftest "pads a short number on the left"
  (assert (= (pad-left "7" 3) "  7")))
```

The emitted Rust is that of 1A under a generated name. A sentence reads well in a report, but a user then types a quoted sentence to run one test.

### Choice 2: finding the tests and running one

#### 2A: a path, the modules it reaches, and a name filter

No new source form.

```sh
rslisp test text.rslisp                # every test of text.rslisp and the modules it uses
rslisp test text.rslisp pads           # tests whose full name contains "pads"
rslisp test text.rslisp --exact text/format/pads-left
rslisp test prelude/tests              # every .rslisp file of the directory
```

`<path>` is a file or a directory. A file is an entry: its module and every user module it reaches through `use` run their tests. A directory makes every `.rslisp` file under it an entry, with the directory as the root of `use`. The filter is a substring of the full name, as in `cargo test`, and `--exact` takes one full name. `rslisp test` passes the filter to the program as its arguments, and the runner applies it, so one build serves every filter.

| The run | Exit code |
| --- | --- |
| every selected test passes, or no test is selected | 0 |
| a test fails or crashes, whatever the program's own exit code or signal | 1 |
| the program has errors | 1 |
| a usage error | 2 |

A non-empty `--untested` list does not change the exit code.

#### 2B: a file only

No new source form. The user names one entry. CI and the prelude then need one call per test file, and each call is one rustc run.

### Choice 3: where tests live

#### 3A: both beside the code and in a separate module

No new source form beyond `deftest`. A `deftest` may sit at the top level of the module of the code it tests, where it sees private names, or of a module of its own that `use`s the code, where it sees only exported names. Both are the same macro, so the rule has no exception. Under `check`, `build`, `emit` and `run`, a test is expanded and type checked and emits nothing, as the probe found, so no flag selects tests.

#### 3B: only in separate test modules, as Go's `_test.go` files

No new source form. `rslisp build` skips files by name. That is a rule about file names that the language has nowhere else, and a test cannot see private names.

#### 3C: kept behind a flag, as Rust's `#[cfg(test)]` and Clojure's `*load-tests*`

```lisp
(defmacro deftest (name body ...)
  (if (testing?) (template ...) (template (do))))
```

The emitted Rust is that of 3A. A new `code` built-in, `(testing?)`, tells a macro whether the command is `test`, and `deftest` expands to `(do)` otherwise. `build` then skips the type check of tests, and a broken test no longer stops a build.

### Choice 4: what `assert` reports

#### 4A: one `assert` that shows both sides of `=`

```lisp
(assert (= (pad-left "7" 3) "007"))
(assert (empty? (pending orders)) "a new order book has no pending orders")
```

```rust
crate::prelude::testing::check_equal(
    "text/format.rslisp:12:3",
    "(assert (= (pad-left \"7\" 3) \"007\"))",
    &pad_left("7", 3),
    "007",
);
crate::prelude::testing::check_true_with(
    "orders.rslisp:30:3",
    "(assert (empty? (pending orders)) \"a new order book has no pending orders\")",
    pending(&orders).is_empty(),
    "a new order book has no pending orders",
);
```

```text
test text/format/pads-left ... FAIL
FAIL text/format/pads-left (text/format.rslisp:12:3)
    (assert (= (pad-left "7" 3) "007"))
    left:  "  7"
    right: "007"
test orders/starts-empty ... FAIL
FAIL orders/starts-empty (orders.rslisp:30:3)
    (assert (empty? (pending orders)) "a new order book has no pending orders")
    a new order book has no pending orders
```

`assert` takes one form and an optional message. When the form is a call of two arguments whose head refers to the prelude's `=`, by `same-binding?`, `assert` evaluates each side once and prints both in the printed form of RFC 0048, so both sides need `show`. Any other form prints as written. The report names sides `left` and `right`, as Rust's `assert_eq!` and ExUnit do, because neither side is the expected one by position. A test without a docstring puts the location right after its name.

`assert` expands to one of four prelude check functions, each of which takes the `assert`'s location and its written text as string literals first:

| Check | Type | Used for |
| --- | --- | --- |
| `check-true` | `(fn ((ref str) (ref str) bool) unit)` | any form other than `=`, without a message |
| `check-true-with` | `(fn ((ref str) (ref str) bool (ref str)) unit)` | any form other than `=`, with a message |
| `check-equal` | `(where ((a (partial-eq b) show) (b show)) (fn ((ref str) (ref str) (ref a) (ref b)) unit))` | `=`, without a message |
| `check-equal-with` | `(where ((a (partial-eq b) show) (b show)) (fn ((ref str) (ref str) (ref a) (ref b) (ref str)) unit))` | `=`, with a message |

`check-equal` compares with `eq`, as `=` expands to `eq`, whose type is `(where ((a (partial-eq b)) b) (fn (a b) bool))`. Its sides may differ in type, as `(= (pad-left "7" 3) "007")` compares a `string` with a `(ref str)`, so its emitted call borrows the `string`.

A failed check panics with `std::panic::panic_any` and a payload that holds its location and its detail, and the runner reads both back. A failed `assert` thus ends its test, as in Rust and Zig. Comparisons other than `=` may show their sides later.

#### 4B: `assert` and `assert-equal`

```lisp
(assert-equal "007" (pad-left "7" 3))
(assert (empty? (pending orders)))
```

The emitted Rust is that of 4A. `assert-equal` shows both values, and `assert` shows only the form. Two names cover what 4A covers with one, and the user must choose the form before writing the test.

#### 4C: show the arguments of every call

```lisp
(assert (> (len names) 3))
```

```rust
{
    let left = names.len();
    let right = 3;
    crate::prelude::testing::check_call("orders.rslisp:8:3", "(assert (> (len names) 3))", left > right, &[&left, &right]);
}
```

`assert` binds each argument of any call and prints all of them, as power-assert does. A macro cannot tell whether an argument's type implements `show`, so `(assert (valid? closure))` would fail to compile with `L0404`.

#### 4D: record a failure and continue, as Clojure's `is` and Go's `t.Errorf`

No new source form. A failed `assert` adds to a list that the runner reads after the test, and the test goes on.

### Choice 5: where a failure's location comes from

#### 5A: a `code` built-in, `(form-location form)`

```lisp
(form-location test)   ; at expansion time: "text/format.rslisp:20:3"
```

The location is a string literal in the emitted Rust, the first argument of each check call. `(form-location form)` returns the form's span as `<file>:<line>:<column>`, with the file relative to the root, and `""` for a form with no span. `assert` passes its own location to its check, whose panic carries it to the runner. `deftest` passes the location of the `deftest` form to `note-test`, and a panic that is not an `assert`, such as an `unwrap` in a test, reports that location.

#### 5B: map the panic's Rust location through `SpanMap`

No new source form. The runtime marks the check function `#[track_caller]`, the runner prints the Rust location of each panic, and `rslisp test` maps it back through the `SpanMap` of `EmittedCrate`. It gives `unwrap` in a test its own line too, and the same mapping could serve `rslisp run`. It parses the program's output, and its precision depends on the span that the emitter records for an expanded macro call, which no probe checked.

### Choice 6: compiling and running the tests

#### 6A: one program, each test under `catch_unwind`, with the runner in the prelude

```lisp
(run-tests :marker "<marker>")
(run-tests :marker "<marker>" :untested prelude)
```

```rust
fn main() {
    crate::prelude::testing::start_run("<marker>", 2_usize);
    crate::prelude::testing::run_test("<marker>", "text/format/pads-left", "", "text/format.rslisp:3:1", test_pads_left);
    crate::prelude::testing::run_test("<marker>", "text/format/trims-right", "Removes trailing spaces, not inner ones.", "text/format.rslisp:6:1", test_trims_right);
    crate::prelude::testing::finish_run("<marker>")
}
```

`rslisp test` builds one program whose `main` is a `run-tests` call, resolved in the prelude's scope as `eval` resolves `println` and `pr-str`. The call is its own source, which the expander expands after every module of the program. `run-tests` is a prelude macro with two keyword arguments:

- `:marker`, a string that `rslisp test` generates for each run, as `eval` generates its value marker.
- `:untested`, a module path, given only with `--untested`. Choice 7 states what it does.

`run-tests` reads the notes under `tests` of every module that `(program-modules)` returns, and expands to one runner call per note. The runner is prelude code: functions and `rust` templates, as the prototype's are. It replaces the panic hook so that a panic prints nothing of its own, then writes **records** to standard output. A record is the marker followed by a printed form, in the printed form of RFC 0048, and a newline. `rslisp test` finds a record anywhere in the stream, as `eval` finds its marker, and holds back an end of the output that could start a marker. These records are the contract between the runner and `rslisp test`, as `print-value` is `eval`'s:

| Record | Written |
| --- | --- |
| `(untested "<module>" <count of exports> "<name> <name> ...")` | first, only with `:untested` |
| `(start <count of selected tests>)` | before the first test |
| `(test "<full name>" "<docstring's first line>")` | before each test, followed by a flush |
| `(ok)` | after a test that returns |
| `(fail "<location>" "<written assert>")`, `(fail "<location>" "<written assert>" "<message>")` | after a failed `assert` on a form other than `=` |
| `(fail-equal "<location>" "<written assert>" "<left>" "<right>")`, with an optional `"<message>"` | after a failed `assert` on `=`, with each side's printed form |
| `(panic "<location of the deftest>" "<message>")` | after any other panic |
| `(done)` | after the last test |

`rslisp test` prints every line of the report from the records, so it alone owns the report's wording and the count line's format. It strips each record from the output it passes through, and prints the program's other output where the program wrote it. It prints a newline before a result when the test's own output did not end with one. Tests run one at a time, in module load order and then source order, so the output is the same on every run and a snapshot can record it.

The count line names `crashed` and `not run` only when a test crashed: `1 passed, 1 failed`, and `1 passed, 0 failed, 1 crashed, 1 not run`.

A test that ends the process, through `exit`, an abort or a stack overflow, leaves a `(test ...)` record with no result after it and no `(done)`. `rslisp test` then prints the crash, the number of tests not run from the `(start ...)` count, and the count line, and exits 1, whatever the program's exit code or signal:

```text
running 3 tests
test crash/first-one ... ok
test crash/crashes ... CRASH
CRASH crash/crashes: the program exited with code 3
1 test not run
1 passed, 0 failed, 1 crashed, 1 not run
```

| The test | The `CRASH` line |
| --- | --- |
| calls `(exit 0)` | `CRASH crash/crashes: the program exited with code 0` |
| calls `(exit 3)` | `CRASH crash/crashes: the program exited with code 3` |
| aborts | `CRASH crash/crashes: the program stopped on signal 6 (SIGABRT)` |
| overflows its stack | Rust's two lines on standard error, `thread 'main' (<id>) has overflowed its stack` and `fatal runtime error: stack overflow, aborting`, then `CRASH crash/crashes: the program stopped on signal 6 (SIGABRT)` |
| is stopped by signal N | `CRASH crash/crashes: the program stopped on signal N (<name>)` |

Cost: one rustc run for any number of tests, about 0.3 s for a small program.

#### 6B: one program per test

No new source form. Isolation is complete, but each test costs a rustc run, about 0.27 s or more. The 221 exported prelude names at one test each would take a minute.

#### 6C: one program, one process per test

No new source form. The same binary runs each test in its own process, chosen by an argument. A crash ends one test only, at about a millisecond per process.

#### 6D: emit `#[test]` and build with `rustc --test`

No new source form.

```rust
#[test]
fn pads_left() {
    assert_eq!(pad_left("7", 3), "  7");
}
```

Rust's libtest catches panics, runs tests in threads and filters by name. The compiler would have to know which functions are tests, which makes it depend on how `deftest` works, and libtest prints Rust names such as `pads_left` in Rust's format.

### Choice 7: static name coverage

#### 7A: written references, matched by binding, in the prelude

No new source form beyond the `:untested` argument of `run-tests`.

```sh
rslisp test prelude/tests --untested prelude
```

```text
untested in prelude (12 of 221): dedup! max-by-key sort-by! ...
running 214 tests
...
```

`rslisp test <path> --untested <module>` builds `(run-tests :marker "<marker>" :untested <module>)`. `run-tests` then collects every symbol of the written bodies in the `tests` notes and lists each exported name of `<module>` that no symbol refers to by `same-binding?`. A name counts when a test writes it, including a macro such as `assert` or `defstruct`, and not when only a macro's expansion introduces it. The list is a string literal that the expansion builds, so the report costs no run. The runner writes it as the `(untested ...)` record, and `rslisp test` prints it before `running N tests`. It is a report: it does not change the exit code, and CI's snapshot records it.

This needs one new built-in, `(module-exports path)`, which returns the exported names of the module at a module path, as `use` writes it. A user program asks for the same report on its own modules: `rslisp test app.rslisp --untested inventory`.

A symbol that a test binds, such as `(let str 1)`, still counts as a reference to the prelude's `str`, because the stored body carries the scopes of the call, not of the `let`. No test in the repository binds a prelude name.

#### 7B: every name the expander resolves inside a test

No new source form. The expander records every binding that a symbol inside a test's expansion resolves to, macros included. The report then counts names that only an expansion introduces, so `(defn f (x) ...)` in a test covers `def` and `fn`, and `assert` covers `check-equal`. It also needs the compiler to know which forms are tests, which makes it depend on `deftest`.

### Choice 8: the prelude's own tests

#### 8A: `prelude/tests/<module>.rslisp`, outside the embedded prelude

No new source form. One test module per prelude module, such as `prelude/tests/text.rslisp` for `prelude/text.rslisp`. `expand/src/modules/sources.rs` embeds an explicit list of prelude files, so these files never reach the compiler binary or the prelude state, and every program's compile time stays the same. They see exported names only, which is what the coverage report counts.

CI runs them through one test in `tests/cli.rs` that runs `rslisp test prelude/tests --untested prelude` and records its output in an insta snapshot. A failing test fails CI, and a new untested export shows in the snapshot diff of the change that adds it. Fixtures stay for diagnostics, emitted Rust and whole-program run output. A run fixture that only checks a prelude function's result MAY move to a test later, case by case.

#### 8B: beside the code in `prelude/*.rslisp`

No new source form. The tests sit next to the definitions. They become part of the prelude state that `build.rs` saves, expanded when the compiler builds and loaded by every compile, so the compiler carries code that no user program runs. They can test private helpers.

## Recommendation

1A, 2A, 3A, 4A, 5A, 6A, 7A and 8A.

- The prelude expresses `deftest`, `assert`, `run-tests` and the runner, which the prototypes show. The compiler gains:
  - three `code` built-ins, `form-location`, `program-modules` and `module-exports`;
  - the `test` command, which builds the `run-tests` call, resolved in the prelude's scope as `eval` resolves `println` and `pr-str`, and reads the runner's records of 6A, a contract that this RFC fixes as `eval`'s marker and `print-value` are fixed;
  - the macro-stage code `L0333` `duplicate-test`;
  - a wording change to `L0208`, below.
- A test is checked by every command and emitted by none but `test`, with no flag, because today's emitter already drops what `main` does not reach.
- One `assert` keeps the user's form as written and still shows the values that matter for `=`.
- One program per run costs one rustc run, which keeps a full prelude run near a second.
- Coverage of written names says what the tests mean to cover, and needs nothing from the compiler but the export list.

**`L0208` names the definer the user wrote.** A macro cannot tell a body from a module level before it expands, so `deftest` cannot check its own position. Today every definer in a body reports `` `def` can only appear at module level `` at the call, a name the user did not write. The build makes `L0208` name the head of the outermost macro call that the user wrote around the `def`, so `(deftest inner ...)` in a body reports `` `deftest` can only appear at module level ``, and `(defn inner () 1)` in a body reports `` `defn` can only appear at module level ``. A `def` that the user wrote keeps its message.

The first build adds `deftest`, `assert`, `run-tests` and the runner to the prelude, the three built-ins, `L0333`, the `L0208` wording, and the `test` command with `--untested`. It adds a fixture for each outcome below that a fixture can record, `tests/cli.rs` tests of the command, its records and its exit codes, and a `prelude/tests/` module for one prelude module as the start. Docstring examples run as tests, as Elixir's doctests do, are a later RFC.

### Outcomes

The recommended options give these outcomes. A row marked probed ran on a prototype.

| Case | Outcome |
| --- | --- |
| (a) `assert` on `=` whose sides lack `show` | `L0404` on each side: ``` `op` does not implement `show` ```, labeled ``this needs `show` ``, with a note that names the field that blocks the derive and the help `a closure, an iterator or a function has no printed form`. Probed on a struct with a function field, which also gets `L0404` for `partial-eq`. |
| (b) `(assert form "message")` | The message prints as the last line of the failure, indented 4 columns, after the written `assert` call and the `left` and `right` lines, with or without a docstring. Probed. An `assert` without a message calls a check with no message argument. |
| (c) a panic outside `assert`, such as `unwrap` | `test <name> ... FAIL`, then `FAIL <name> (<location of the deftest>)`, then the panic's message indented 4 columns. For `unwrap` the message is Rust's ``called `Option::unwrap()` on a `None` value``. Probed: the `(panic ...)` record carries the `deftest`'s location and the message. |
| (d) a filter that matches no test, or a program with no tests | `running 0 tests`, then `0 passed, 0 failed`, exit 0. A filter that matches no test also prints ``note: no test name contains `<filter>` `` on standard error. |
| (e) `--untested m` for a module the program does not load | A usage error before the build, ``error: the program does not load module `m` ``, exit 2. |
| (f) `deftest` inside a function body, or inside a macro expansion | In a body, `L0208`, ``` `deftest` can only appear at module level ```, at the `deftest` call. Today's compiler names `def` there (probed). A user macro that expands to `deftest` at module level defines a test under the written name it passes (probed). |
| (g) `(doc pads-left)` for a test `pads-left` | The page of the user's `pads-left` if one exists, otherwise `L0201`. The docstring of a test goes only into its note. Probed: `(doc-page twice-named)` for a test `twice-named` is `L0201`. |
| (h) a test that reads standard input | It reads the standard input of `rslisp test`, as a program under `run` reads its own, in test order. A test that reads a line and prints it with `prn` shows `test reads ... (ok (some "line one"))`, then `ok` on the next line. One that prints it with `print`, with no final newline, shows the same two lines: `rslisp test` adds the newline before `ok`. Probed: the program's own output, `(ok (some "line one"))`. |
| (i) a test that prints | Its output follows `test <name> ... ` on the same line, and the result starts the next line. `(println "hello")` and `(print "hello")` both show `test prints ... hello`, then `ok`. Probed: `(print "hello")` leaves `hello<m>(ok)` in the stream. |
| (j) an entry with its own `main` | As under `eval`: `rslisp test` checks the module's `main` and does not run it, and the generated `main` is the entry. A test that calls `main` calls it as `main_2` in Rust. |

### Later: runtime coverage

Line or branch coverage needs counts from the run. Two paths exist, both for a later RFC:

- **The emitter counts.** It adds a counter to each `if` branch, `match` arm and function body in a test program, and the runner prints the counts with the spans they belong to. It needs nothing outside the repository, and the counters stay out of every other command's Rust.
- **rustc counts.** `rustc -C instrument-coverage` writes a profile, which `llvm-profdata` and `llvm-cov` turn into regions of the emitted Rust, and `SpanMap` maps them back to source. It needs the LLVM tools in the dev shell, and its regions follow Rust's blocks, not the source's forms.

Either path reports a form inside a prelude macro's expansion at the macro call, so a report on the prelude itself needs the expansion's spans as well.

### Prior art

- **Clojure** ([`clojure.test`](https://clojure.github.io/clojure/clojure.test-api.html)): `deftest` defines a var whose metadata holds the test. `is` is one macro that reports `expected`, the form, and `actual`, the evaluated call, for any predicate call, and special-cases `thrown?` and `thrown-with-msg?`. `*load-tests*` set to false makes `deftest` expand to nothing. A failed `is` does not stop its test.
- **Racket** ([RackUnit](https://docs.racket-lang.org/rackunit/), [`module+ test`](https://docs.racket-lang.org/guide/Module_Syntax.html#%28part._main-and-test%29)): `check-equal?` and its siblings are separate forms. Tests live in a `test` submodule beside the code, which `raco test` runs and a normal run of the module never loads.
- **Rust** ([`#[test]`](https://doc.rust-lang.org/book/ch11-01-writing-tests.html), [`cargo test`](https://doc.rust-lang.org/cargo/commands/cargo-test.html)): tests sit beside the code under `#[cfg(test)]` and see private items. libtest builds one binary per test target, runs each test on its own thread, and filters by substring. `assert_eq!` prints `left` and `right`.
- **Zig** ([`test` blocks](https://ziglang.org/documentation/master/#Zig-Test)): `test "name" { ... }` sits in any file, and a normal build leaves it out. `zig test` builds one binary of the tests that the root file reaches, with `--test-filter`. `std.testing.expectEqual` returns an error rather than panicking.
- **Go** ([`testing`](https://pkg.go.dev/testing), [`go test`](https://pkg.go.dev/cmd/go#hdr-Test_packages)): `TestXxx` functions in `_test.go` files, which `go build` skips, run with `-run regexp`. `t.Errorf` records a failure and continues. `go test -cover` instruments the source.
- **Elixir** ([ExUnit](https://hexdocs.pm/ex_unit/ExUnit.html), [`assert`](https://hexdocs.pm/ex_unit/ExUnit.Assertions.html), [doctests](https://hexdocs.pm/ex_unit/ExUnit.DocTest.html)): `test "name" do ... end` with a string name, one `assert` macro that reads its expression and shows `left` and `right` for `==` and other operators, `mix test file:line` to run one test, and doctests that run the `iex>` examples of a docstring.

## Decision

The project owner accepted the recommendation on 2026-11-05, with these answers:

1. **Test names: 1A.** A test is a symbol name with an optional docstring. Its function has an introduced name, so no user name collides with it and `check` and `signatures` leave it out. A second test of one name in a module is `L0333`, ``test `pads` is defined more than once``, at the second `deftest`, with the first marked. A passing test prints one line, `test text/format/pads-left ... ok`. A failure prints the first line of the docstring after the name, as `FAIL text/format/trims-right: Removes trailing spaces, not inner ones. (text/format.rslisp:20:3)`, then the form and `left` and `right`. The run ends with a count line such as `2 passed, 1 failed`. Choice 1 states the output.
2. **Where tests live: 3A.** A test may sit at the top level of any module. Every command checks it, and only `test` emits it. No flag selects tests.
3. **A failed `assert` ends its test.**
4. **One `assert`, 4A.** It shows both sides for `=` only. Other comparisons may join later.
5. **Location: 5A,** the `form-location` built-in.
6. **Running: 6A.** One program runs every test. A crash names its test, and the tests after it are not run.
7. **Coverage: 7A.** Coverage counts the names that a test writes.
8. **The prelude's tests: 8A.** They live in `prelude/tests/`.

## Rejected options and why

- **1B, a plain function under the written name:** a test named after its function redefines the function.
- **1C, a string name:** running one test means typing a quoted sentence. The optional docstring gives a failure its sentence instead.
- **A function named by a symbol that the reader cannot produce, such as `test pads-left`:** the name is a user binding, so `rslisp signatures` prints `(type test pads (fn () unit))`, a user function `test-u20-pads` clashes with it in Rust with `L0602` under `test` alone, and its `L0203` names `test pads`, a name the user never wrote.
- **2B, a file only:** CI and the prelude would pay one rustc run per test file.
- **3B, separate files by name:** a rule about file names, and no private names in tests.
- **3C, a `testing?` flag:** the emitted Rust is the same without it, so it would buy only a skipped check, at the price of a built-in and a program that checks differently under two commands.
- **4B, `assert` and `assert-equal`:** two names for what one `assert` covers, and the user must choose before writing the test.
- **4C, every argument shown:** a macro cannot know whether an argument implements `show`.
- **4D, record a failure and continue:** the later asserts of a test often depend on the earlier ones, and a panic that ends the test needs no state shared between `assert` and the runner.
- **5B, panic locations mapped through `SpanMap`:** it parses the program's output, and its precision depends on the span that the emitter records for an expanded macro call.
- **6B, one program per test:** a rustc run per test.
- **6C, one process per test:** a crash is rare, and the one-program runner can move to it later without changing a test.
- **6D, `rustc --test`:** the compiler would depend on `deftest`, and the report would print Rust names.
- **The runner printing the report itself:** `rslisp test` must print the crash lines and the final count after a crash, so the wording would live in two places.
- **7B, every name the expansion resolves:** it counts names that only a macro introduces, such as `def` under `defn`, and it makes the compiler depend on `deftest`.
- **8B, tests in the embedded prelude:** the compiler binary and every compile would carry tests that no user program runs.

## Rule

The build that lands this design adds a Tests section to [docs/language.md](../docs/language.md), next to Evaluation, that states `deftest`, `assert`, `rslisp test`, its records, its output and its exit codes. It adds the three new built-ins to the table of [The `code` module](../docs/language.md#the-code-module), the header shape of `deftest` under [Header shapes](../docs/language.md#header-shapes), and `L0333` and the new wording of `L0208` to `docs/diagnostics.md`.
