# RFC 0056: Amendments to tests written in the language

**Status:** Accepted
**Date:** 2026-11-05

## Problem

The build of RFC 0055 showed four things that the RFC does not hold.

**Every compile paid for the test runner.** With `deftest`, `assert`, `run-tests` and the runner in the always-loaded prelude, `tests/fixtures/corpus/empty_program`, which has no tests, rose 37% in instructions, 32% in allocated bytes and 17% in peak live bytes. `warehouse` rose 8%, 8% and 12%. The runner's `defn` bodies caused most of it: `check.inferred_expressions` on `empty_program` went from 4 to 148. The checker gives a `(def name (rust ...))` its type with no inference, and the rest of the prelude is such templates, except `sum`. A written signature does not skip a body: `sum` has one, and its body is the 4.

With the runner moved to a runtime part, `empty_program` was still 7.3% above `main` in instructions and 8.4% in peak live bytes, and `warehouse` 1.3% and 4.0%. `prelude/testing.rslisp` was 9.8 KB of a 119 KB prelude, so it cost what any prelude module costs per byte. Only code outside the saved prelude state pays nothing.

**`check-equal` with `(ref a)` and `(ref b)` does not type check as `=` does.** With the `(where ((a (partial-eq b) show) (b show)) ...)` of RFC 0055, a literal or an open type behind `(ref b)` never meets the other side: `(assert (= (len names) 2))` and `(assert (= found none))` are `L0404`.

**A `let` for each side does not keep outcome (a) of RFC 0055.** Comparing two bound names type checks as `=` does. A plain `let` moves a variable, so `(assert (= names others))` makes a later use of `names` `L0501`. A `let` of a borrow keeps the value, but the names take the location of the `assert`, and no `code` built-in gives a symbol another form's span. A side without `show` is then `L0404` twice on the whole `assert`, not once on each side.

**A written `deref` turns off the borrow at a call**, as `docs/language.md` states, so `(deref item)` cannot meet a `(ref a)` parameter, though a written `=` accepts it.

**`assert` failed to build outside a test program.** One runtime part held the checks and the runner, and only `crate::rt::start_run(` selected it, so `assert` in a helper that `main` reaches passed `rslisp test` and was `E0425` under `build`, `run`, `eval` and the prompt. That part also put `#[allow(dead_code)]` in emitted Rust, which no other part does. A check that panics with a private payload prints `Box<dyn Any>` under the default panic hook.

**A test's value reached the generated call.** With `run-test` over `(fn () a)`, `(deftest open none)` passed `check` and was `L0407` on `<test runner>` under `test`.

## Options

### Where `deftest`, `assert` and `run-tests` live

#### A: a prelude module that a test module imports

```lisp
(use prelude/test assert deftest)

(deftest pads-left
  (assert (= (pad-left "7" 3) "  7")))
```

`prelude/test` is a prelude file that `prelude/prelude.rslisp` does not import, so it stays out of the saved prelude state and loads from source on its first `use`, as any module does. A program with no tests pays nothing. A test module writes one `use`, as a Clojure test file requires `clojure.test`.

#### B: only `run-tests` and the coverage helpers leave the prelude

No new form in a test. The estimate from file size is `empty_program` near 4.7% above `main`, still over the 1% that the memory gate allows.

#### C: keep everything in the prelude and accept the rise

No new form. Every compile keeps paying about 7%.

### How `assert` passes the two sides of `=`

#### A: the sides are arguments, and the comparison is a closure

```lisp
(check-equal "text/format.rslisp:12:3"
             "(assert (= (pad-left \"7\" 3) \"007\"))"
             (pad-left "7" 3)
             "007"
             (fn (left right) (= left right)))
```

```rust
crate::rt::check_equal(
    "text/format.rslisp:12:3",
    "(assert (= (pad-left \"7\" 3) \"007\"))",
    &pad_left("7", 3),
    "007",
    |left: &String, right: &str| left == right,
    None,
)
```

Each side keeps its span and runs once, a side keeps its value, and the closure's `=` type checks as a written `=` does. `L0404` for `show` lands once on each side, and for `partial-eq` once.

#### B: a `let` of a borrow for each side, and a `bool` to the check

`L0404` for `show` lands twice on the `assert`, a doubled diagnostic.

#### C: a new `code` built-in that gives a symbol a form's span

Keeps option B's expansion, and adds to the compiler.

## Recommendation

`prelude/test` with an explicit `use`, and the closure.

## Decision

The project owner decided on 2026-11-05:

1. **`deftest`, `assert`, `run-tests` and the coverage helpers live in `prelude/test`,** which the always-loaded prelude does not import. A test module writes `(use prelude/test assert deftest)`.
2. **`rslisp test` resolves `run-tests` in `prelude/test`,** which it loads itself.
3. **`check-equal` and `check-equal-with` take the two sides and the comparison as a closure.** Their types are `(where ((a show) (b show)) (fn ((ref str) (ref str) (ref a) (ref b) (fn ((ref a) (ref b)) bool)) unit))` and, with a message, the same with `(m display)` and a last `(fn-once () m)`.
4. **The checks and the runner are runtime parts,** one per marker in `prelude/runtime/`, with `rust` templates in `prelude/test`, as `expect-ok` is. The emitted Rust holds a function only when it calls that function, and no `#[allow(dead_code)]`.
5. **A side of `=` that is a `deref`, written or from a macro's expansion, passes to the check as `(do side)`.** `=` accepts such a side only when its value is Copy, and the call borrows the block's value, so `assert` accepts and rejects what `=` does, and each side runs left then right. `(ref (deref x))` emits `&*x`, which `borrow_deref_ref` rejects. Passing `x` itself fails on a reference to a reference with rustc's `E0631` and accepts `(deref x)` of a value. A `let` of the side runs a right `deref` before the left side. A macro cannot see `x`'s type. A Copy side without `show` is `L0404` on the `assert`, once, not on the side.
6. **`assert` works in any code,** as Rust's `assert!` does. A failed check stores its record in a static of the runtime, where the runner finds it, and panics with its location, its written form, both sides and its message, so the panic reads with no runner.
7. **A test's function ends with a unit form, and `run-test` takes `(fn () unit)`.** A test's value is dropped, so a test that returns an `err` passes.
8. **A mismatch between the two sides labels the whole `assert`.** A written `=` labels the right side. The closure's `left` and `right` take the `assert`'s location, and a macro cannot give them a side's span: the closure's head written as the user's own `=` symbol labels the same place.
9. **The message runs only when the check fails,** as with Rust's `assert!`. `assert` passes it as a closure, a string literal as it is and any other message under `str`, so a variable keeps its value.

## Rejected options and why

- **`run-tests` alone out of the prelude:** it adds a design and still misses the 1% that the gate allows.
- **Accepting the rise:** every compile would pay for a runner that it does not run.
- **A `let` for each side:** a doubled diagnostic.
- **A built-in that sets a symbol's span:** it adds to the compiler what the prelude can express.
- **Documenting a `deref` side as an error:** `assert` would reject a form that `=` accepts.
- **`(ref (deref x))` for a `deref` side:** it emits `&*x`.
- **`x` itself for a `deref` side:** it is rustc's `E0631` on a reference to a reference, and it accepts a `deref` of a value.
- **A `let` for a `deref` side:** it runs a right `deref` before the left side.

## Rule

[docs/language.md](../docs/language.md#tests) states the current rules of `prelude/test`, `assert`, `check-equal` and the runner.
