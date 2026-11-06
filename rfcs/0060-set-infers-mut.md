# RFC 0060: `set!` on a parameter makes it `mut`

**Status:** Accepted
**Date:** 2026-11-06

## Problem

A `set!` of a whole parameter changes the caller's value for some types and not for others:

```lisp
(defn bump! (x) (set! x (+ x 1)))
(defn replace! (items) (set! items (vec 4 5)))

(defn main ()
  (local total 5)
  (bump! total)
  (println total)
  (local numbers (vec 1 2 3))
  (replace! numbers)
  (println (len numbers)))
```

```rust
fn bump(mut x: i64) {
    x = x + 1
}

fn replace(items: &mut Vec<i64>) {
    *items = vec![4_i64, 5_i64]
}
```

`total` stays `5`, and `numbers` changes. The name says `bump!` changes its argument, `L0504` stays silent, and the write is lost. The type of the argument decides what `set!` means, and nothing on the screen says so.

A parameter written `(mut i64)` does not fill the gap: `(set! x (+ x 1))` on it is `L0401`, from `(+ x 1)`, because the copy-out adjustment does not reach a `mut` value. The body has to write `(deref x)`, so the inferred and the written forms read differently.

A function value hides the same loss. With `(type bump! (fn (i64) unit))`, the call `(apply-change bump! (mut total))` compiles through `|argument: &mut i64| bump(*argument)` and leaves `total` as it was.

## Options

Every example in this RFC was checked against a release build of `main` at `e31f99e3`, against a throwaway prototype on that commit, or carries the words "after the build". The build of this RFC follows the build of RFC 0058, so the examples write `local`. `main` does not hold `local` yet, so each one was checked as today's flat `let`.

### A: `set!` infers `mut` for every type

```lisp
(defn bump! (x) (set! x (+ x 1)))

(defn main ()
  (local total 5)
  (bump! total)
  (println total))
```

```rust
fn bump(x: &mut i64) {
    *x = *x + 1
}

fn main() {
    let mut total: i64 = 5;
    bump(&mut total);
    println!("{}", total)
}
```

The prototype prints `6`. The caller writes nothing, and the `!` in the name is the marker at the call.

### B: a warning only

`set!` keeps its meaning, and a Copy parameter that a `set!` changes reports a warning.

```lisp
(defn bump! (x) (set! x (+ x 1)))
```

```rust
fn bump(mut x: i64) {
    x = x + 1
}
```

The Rust stays as it is, and `bump!` still cannot change an `i64`, whatever the warning says.

### C: `mut` required at the call

The caller writes the borrow, as Rust, Swift and C# ask.

```lisp
(bump! (mut total))
```

```rust
bump(&mut total);
```

Every call of a function that changes an argument gains a form, and the compiler already knows where each borrow goes.

## Recommendation

Option A. One rule covers every type, the emitted Rust is what a Rust programmer writes for a function that changes an `i64`, and inference stays. The cost is that a body that used `set!` on a Copy parameter as scratch space now changes its caller's value. The Rule section counts those bodies: there are none in this repository.

## Decision

The project owner decided on 2026-11-06:

1. A `set!` of a whole parameter counts as a `mut` use for every type, Copy types included. The parameter is inferred `(mut a)`, and the caller writes nothing: `(defn bump! (x) (set! x (+ x 1)))` called as `(bump! total)` emits `fn bump(x: &mut i64) { *x = *x + 1; }` and `bump(&mut total)`, and `total` changes.
2. Inference stays. A caller never has to write `mut`, and a written `(mut total)` stays allowed and checked.
3. The `!` in a name is the visible marker at the call. `L0504` keeps warning on a function that changes an argument without `!`. A new warning covers the converse: a `!` function that changes no argument.
4. The copying wrapper that adapts a by-value function to an expected `(fn ((mut a)) ...)` stays allowed only when the function does not change its parameter. When it does, which after decision 1 only a written by-value signature allows, the compiler reports `L0506` at the function value, with a help to write the parameter as `(mut i64)`.
5. Options B and C are not taken. Before 1.0 the owner is the only user, so the change needs no migration aid, and its release is minor.

The project owner decided the rest on 2026-11-06:

6. A closure's own parameter and a `loop` binder stay by value: a `set!` on them updates the local copy, as Rust's `|mut x|` and `let mut n` do. A closure changes an outside variable by capturing it.
7. `(local n n)` keeps the name `n` in the emitted Rust, `let mut n = n;`, whenever the old binding is not read again after it.
8. A wrapped Rust type declares interior mutability with a marker on its definition, as `(deftype (cell a) (rust "std::cell::Cell<{0}>") :interior-mutable)`. Passing a value of such a type counts as a possible change for both `L0504` and `L0508`, so neither warns falsely.
9. An `at` element is a place a function can change: `(bump! (at scores 0))` emits `bump(&mut scores[0])` and changes the element.
10. `recur` with a new value for a parameter that decision 1 made `mut` writes through and loops: `(recur (+ x 1))` emits `*x = *x + 1; continue;`, so the caller sees the final value. `recur` gives each parameter its new value as `set!` does.
11. An unbounded type parameter that a `set!` changes from its own value infers a `copy` bound, by the existing rule that a body copying a generic parameter infers `copy`: `(apply! count inc)` changes `count`, and `(apply! name trim)` on a string is an error at that call with a help. No clone is inserted.

## Rejected options and why

- **B: a warning only:** a warning names the loss and keeps it. No program could write `bump!` for an `i64` without a written signature and a `deref`.
- **C: `mut` required at the call:** it adds a form to every such call and repeats what the `!` in the name says, and it gives up inference, which the language keeps everywhere else.
- **`mut` on a closure's own parameter:** a closure takes its expected type from the call it is passed to, and a `loop` binder is such a parameter, so telling them apart needs an exception. A capture already changes an outside variable.
- **An error at `(recur (+ x 1))` after a `set!`:** it stops a body that compiles on `main`, and `recur` already means "give each parameter its next value".
- **An unbounded type parameter kept owned:** the `i64` caller would lose the write with no report, which decision 1 exists to end.
- **An error at the definition of such a parameter:** it asks every `i64` caller's function for a written signature, where the existing `copy` rule gives the same program with nothing written.

## Rule

The build states these rules in [docs/language.md: Ownership](../docs/language.md#ownership), [8.1 References at call sites](../docs/language.md#81-references-at-call-sites), [8.4 Loops](../docs/language.md#84-loops), [Rust interop](../docs/language.md#rust-interop), [Names](../docs/language.md#names), the `rust-type` row of [Kernel forms](../docs/language.md#kernel-forms) and the page of `set!`. It rewrites the Modes, Adjustment, Function values, Reborrows, Mut borrows, Assignment and Closures bullets of Ownership, and the sentence of Status that a written `mut` turns off adjustment keeps its meaning for a written `(mut x)` form. This RFC supersedes `D1` for a name of type `(mut a)`.

### 1. What breaks

A `set!` of a whole parameter, across `prelude/`, `examples/` and `tests/fixtures/`:

| Type | Sites | Files | Today |
| --- | --- | --- | --- |
| `string` | 4 | `set_mut_parameter` and `set_mut_parameter_reading_itself`, in `emit` and `run` | already `mut` |
| `(vec i64)` | 2 | `set_mut_parameter_vector`, in `emit` and `run` | already `mut` |
| a Copy type | 0 | | |

All six are `defn` parameters, and none is in the prelude or an example. The five other `set!` forms of a bare name in those folders change locals. **No site is a local scratch update, so decision 1 turns no existing `set!` into a change the caller sees.**

The prototype implements the inference, most reads of rule 4 and the warning of rule 6, without its exceptions. Its `emit` output, with diagnostics, matches `main` on 1,836 of the 1,838 files under `examples/` and `tests/fixtures/`, leaving out `read/` and `format/`. The two others are `check/clone_meaning`, which gains the warning of rule 6 on `twice!`, and `check/generic_mut_beside_literal`, whose first error moved from `L0401` to `L0419` only because the prototype's change to shared parameters also reaches a written `(mut year)`. The build keeps that snapshot, by rule 4.

### 2. The modes

**A `set!` of a whole parameter is a `mut` use. So is passing the parameter to a `mut` parameter, and so is a written `(mut x)`.** Today a Copy parameter takes neither of the first two as a reason to borrow:

```lisp
(defn change! ((x (mut i64))) (set! x 9))
(defn pass-on! (x) (change! x) x)
```

```rust
fn pass_on(x: &mut i64) -> i64 {
    change(x);
    *x
}
```

On `main` this emits `fn pass_on(mut x: i64)`, and `change(&mut x)` changes the copy.

**A read of a Copy parameter needs no ownership, whatever its size.** Today such a read counts as an owned use, and an owned use outranks `mut`, so without this rule `(+ x 1)` would keep `x` by value. The size limit of the Adjustment bullet does not apply, because a copy the source reads costs no more through a borrow, and with the limit a `(pair i64 i128)` would lose the write: `(defn step! (p) (set! p (flip p)))`, with a user function `flip`, emits `fn step(mut p: (i64, i128))` on `main`, and `fn step(p: &mut (i64, i128))` with `*p = flip(*p)` after the build. A type parameter that allows only Copy values follows the same rule: `(defn twice! (x) (set! x (+ x x)))` emits `fn twice<A: crate::rt::Number>(x: &mut A)`.

**A use that moves a value that is not Copy still outranks `mut`,** as the ladder of modes says:

```lisp
(defn append (s) (set! s (str s "x")) s)
```

```rust
fn append(mut s: String) -> String {
    s = format!("{}x", s);
    s
}
```

The caller gave the value up, and any later use of it is `L0501`, so a value that is not Copy loses no write in silence.

**An unbounded type parameter that a `set!` changes from its own value infers `copy`.** The body copies the parameter, so the existing rule of the Copy types bullet applies, and the read then needs no ownership. After the build:

```lisp
(defn apply! (x f) (set! x (f x)))
(defn inc (n) (+ n 1))
```

```rust
fn apply<A: Copy>(x: &mut A, f: impl FnOnce(A) -> A) {
    *x = f(*x)
}
```

`(apply! count inc)` changes `count`. On `main` this emits `fn apply<A>(mut x: A, ...)`, and an `i64` caller loses the write. No clone is inserted, so a value that is not Copy is an error at the call, the existing `L0404` with its labels and a help that names the function:

```text
error[L0404]: `string` does not implement `copy`
 --> src/main.rslisp:6:11
  |
1 | (defn apply! (x f) (set! x (f x)))
  |                               - this copies `x`
...
6 |   (apply! name shout)
  |           ^^^^ this needs `copy`
  |
  = help: to copy a value that is not `copy`, write `(clone ...)` around it in `apply!`
```

That report needs a function from `string` to `string`. `(apply! name trim)` fails earlier: `trim` takes and returns a `(ref str)`, so `main` reports a bare `L0401` at `trim`, `expected (fn (string) string), found (fn ((ref str)) (ref str))`. The build adds a help to that report when the function's parameter is a view of the value's type:

```text
  = help: `apply!` needs a function that takes and returns `string`; change the value at the call: `(set! name (str (trim (as-str name))))`
```

A `set!` that does not read the parameter asks for no bound: `(defn swap-in! (x y) (set! x y))` keeps `fn swap_in<A>(x: &mut A, y: A)`, for a `string` too.

**A written signature stays exact.** A written by-value parameter keeps its local meaning, for `(defn hide ((x i64)) ...)` and `(type hide (fn (i64) unit))`, and emits `mut x: i64`. A written type parameter without `copy` gets no inferred bound, so the read is the existing error that suggests `(a copy)`.

**A closure's own parameter and a `loop` binder stay by value, for every type.** A `set!` on one updates the local copy, as Rust's `|mut x|` and `let mut n` do:

```lisp
(local f (fn (x) (set! x (+ x 1)) x))
```

```rust
let f = |mut x: i64| {
    x = x + 1;
    x
};
```

`(f total)` leaves `total` as it was, and `(fn (t) (set! t ...))` on a `string` emits `|mut t: String|`. A closure changes an outside variable by capturing it, as rule 8 shows.

### 3. A local update

A body that wants a value of its own binds a copy first:

```lisp
(defn clamp (x)
  (local limited x)
  (if (> limited 10) (set! limited 10) (do))
  (println limited))
```

```rust
fn clamp(x: i64) {
    let mut limited: i64 = x;
    if limited > 10 {
        limited = 10
    }
    println!("{}", limited)
}
```

A value that is not Copy clones: `(local mine (clone s))` keeps `s` a borrow, `s: &String`, and `(local mine s)` moves it.

**`(local n n)` keeps the name `n` whenever the old binding is not read again after it.** A body that needs no new name writes the shorter form, and the emitter gives what hand-written Rust writes, after the build:

```lisp
(defn clamp (x)
  (local x x)
  (if (> x 10) (set! x 10) (do))
  (println x))
```

```rust
fn clamp(x: i64) {
    let mut x = x;
    if x > 10 {
        x = 10
    }
    println!("{}", x)
}
```

On `main` this emits `let mut x_2: i64 = x;`. The value names its own type, so the form takes no annotation. A flat `local` hides the old binding for the rest of the body, so only a borrow or a closure made before it can read the old one, and either counts as a read while it is live, as under RFC 0058's rule for a `loop`'s bindings. A scoped `(let (n n) ...)` followed by a read of `n` keeps its renamed binding.

The mistakes on the way, each after the build:

| Mistake | Report |
| --- | --- |
| `set!` on the parameter of a plain name | `L0504`, with the helps below |
| The call passes a value that is not a place, as `(clamp 12)` | `L0514`, by rule 5 |
| `(local mine s)` on a value that is not Copy, then a use of `s` | `L0501`, whose help suggests `(clone s)` |
| A written by-value signature, on a `!` name | `L0508`, by rule 6 |

`L0504` keeps its title and adds a label on the `set!` and a second help. It names the parameter when a `set!` of a whole parameter caused the warning:

```text
warning[L0504]: `clamp` changes an argument, so its name should end with `!`
 --> src/main.rslisp:1:7
  |
1 | (defn clamp (x)
  |       ^^^^^ this function changes an argument
2 |   (if (> x 10) (set! x 10) (do))
  |                 ---- this changes the caller's value
  |
  = help: rename it `clamp!`
  = help: to change only this function's copy, start the body with `(local x x)`
```

The second help reads `(local x (clone x))` for a value that is not Copy.

**`recur` needs nothing from this rule.** A parameter that `recur` rewrites and no `set!` touches stays as it is, so `(defn count-down (n) (if (> n 0) (recur (- n 1)) n))` keeps `fn count_down(mut n: i64)` and `n -= 1;`. A `loop` binder is a parameter of a `fn` that is called where it is written, so it stays a local, by rule 2, and `(set! acc (+ acc 1))` inside a `loop` keeps `acc = acc + 1`.

When a body does both, `(recur x)` passes the borrow on and emits `continue;`:

```rust
fn f(x: &mut i64) -> i64 {
    loop {
        *x = *x + 1;
        if *x < 5 {
            continue;
        } else {
            return *x;
        }
    }
}
```

**`recur` gives a `mut` parameter its new value as `set!` does, and loops.** A new value for a Copy parameter that a `set!` changes writes through, so the caller sees the final value, after the build:

```lisp
(defn g! (x)
  (set! x (+ x 1))
  (if (< x 5) (recur (+ x 1)) x))
```

```rust
fn g(x: &mut i64) -> i64 {
    loop {
        *x = *x + 1;
        if *x < 5 {
            *x = *x + 1;
            continue;
        } else {
            return *x;
        }
    }
}
```

On `main` this emits `fn g(mut x: i64)`, and no file in this repository holds such a body. A `recur` is not an argument to a function, so `L0514` does not apply to its new value. A tail call of the function itself compiles to the same loop and follows the same rule. A plain call of the function in any other position passes a copy, so it is `L0514`. A written `(mut i64)` parameter reads the same: `(recur (+ (deref x) 1))` on one is `L0419` on `main` and writes through after the build. A `recur` that changes several parameters assigns them together, as it does today: `(recur (+ x 1) (+ n 1))` with a by-value `n` emits `(*x, n) = (*x + 1, n + 1); continue;`.

A parameter that `recur` rewrites and that nothing changes stays by value, as above. A parameter that is not Copy stays owned, as rule 2 says.

### 4. A written `(mut i64)` parameter

**A name whose type is `(mut a)`, where `a` is a Copy type, reads as a copy where the expected type is not a reference, and emits `*x`.** The inferred and the written forms emit the same Rust:

```lisp
(type bump! (fn ((mut i64)) unit))
(defn bump! (x) (set! x (+ x 1)))
```

```rust
fn bump(x: &mut i64) {
    *x = *x + 1
}
```

The rule reaches a shared type parameter, as `(+ x 1)` and `(= x y)` do, and a Rust template, an `if` test and a `local` value with no written type:

```lisp
(type reads (fn ((mut bool) (mut i64)) i64))
(defn reads (flag x)
  (local y x)
  (println (+ y 1))
  (if flag (+ x 1) 0))
```

```rust
fn reads(flag: &mut bool, x: &mut i64) -> i64 {
    let y: i64 = *x;
    println!("{}", y + 1);
    if *flag { *x + 1 } else { 0 }
}
```

An `if` test also takes the copy out of a `ref`: a `(ref bool)` test is `L0401` on `main`, and the same adjustment covers both. Other positions, such as an item of `(vec x)`, read the same way after the build.

**The name keeps its borrow everywhere else,** as it does on `main`, so each of these emits what `main` emits:

| Use | Form | Emits |
| --- | --- | --- |
| `set!` target | `(set! x 9)` | `*x = 9` |
| expected `(mut ...)` | `(change! x)` | `change(x)` |
| expected `(mut ...)` in a result | `(defn f (x) x)` at `(fn ((mut i64)) (mut i64))` | `x` |
| expected `(mut ...)` in a payload | `(take (some x))` at `(option (mut i64))` | `take(Some(x))` |
| expected `(ref ...)` | `(show x)` at `(ref i64)` | `show(x)` |
| a branch | `(change! (if c a b))` | both borrows |
| a `recur` argument | `(recur x)` | `continue` |
| `deref` operand | `(deref x)` | `*x`, not `**x` |
| a receiver | `(len s)` on `(mut (ref str))` | `s.len()` |
| a field base | `(:year c)` on `(mut car)` | `c.year` |
| `match` scrutinee | `(match x ...)` | `match x` |

A branch takes the expected type of its `if` or `match`, so a branch at a value type reads as a copy. A `match` has no expected type and keeps the borrow, because `D10` binds each name of a `(mut t)` as a `mut`:

```lisp
(type f! (fn ((mut (option i64))) unit))
(defn f! (x) (match x ((some v) (set! v 7)) (none (do))))
```

```rust
fn f(x: &mut Option<i64>) {
    match x {
        Some(v) => *v = 7,
        None => {}
    }
}
```

`D1` said that a written `mut` turns adjustment off. This rule supersedes `D1` for a name of type `(mut a)`, and a written `(mut x)` form stays exact.

**On a name of type `(mut a)`, a written `(mut x)` is a reborrow, for the written and the inferred form alike.** Elsewhere a written `(mut x)` stays exact, so 8.1 still holds for it and `generic_mut_beside_literal` keeps its errors. Decision 2 needs both sides: a caller may write what inference finds, and a body may write it too.

| Source | `main` | After the build |
| --- | --- | --- |
| `(bump! (mut total))` | `L0506` | `bump(&mut total)` |
| `(show (mut a))`, where `show` changes nothing | `L0506` | `L0506` |
| `(defn f! ((x (mut i64))) (change! (mut x)))` | `L0401`, `found (mut (mut i64))` | `change(x)` |
| `(defn f! (x) (change! (mut x)) x)` | `fn f(mut x: i64)`, `change(&mut x)` | `fn f(x: &mut i64) -> i64`, `change(x)`, `*x` |
| `(defn f! (x) (local p (mut x)) (set! p 1) (println x))` | `fn f(x: &mut i64)` | the same |
| `(local p (mut x))` then `(set! p 1)`, on a written `(mut i64)` | `L0419` | `let p = &mut *x; *p = 1` |

The fourth row is the prototype's output, and the last two forms now read the same.

A local that holds a borrow keeps it too: after `(local p (mut a))` on a local `a`, `(set! p 1)` emits `*p = 1`.

A bare name in a `local` changes one program shape. On `main`, `(local p x)` moves the borrow of a written `(mut i64)`, and `(set! p 1)` then changes the caller's value through `let p = x; *p = 1`. After this rule `p` is a copy, as it is for an inferred `mut`, and the same two forms emit `let mut p: i64 = *x; p = 1`. To change the caller's value, the body writes `(set! x 1)` or `(local p (mut x))`. No file in this repository holds such a body.

### 5. Arguments that cannot change

**An argument at a `mut` parameter must be a place, which is a local, a field or an `at` element, or a value of type `(mut a)`, whether the mode is written or inferred.** The second half is `main`'s own exemption: `(change! (f (mut a)))` emits `change(f(&mut a))` and stays allowed.

`L0419` stays the report for a written form, at the type stage: a written `(mut 5)`, and an argument at a written `(mut i64)` parameter. Only the ownership stage knows an inferred mode, and `docs/diagnostics.md` groups codes by stage, so the inferred check takes a new code, `L0514`, `INFERRED_MUT_OF_COPY`. Without it, `(bump! 5)` on an inferred `mut` emits `bump(&mut 5)` on the prototype, which rustc accepts and which changes nothing. `(grow! (vec 0))` does the same on `main` today.

Both codes share the title and the help below. The help stops saying that a call returned a copy, because a literal is no call:

```text
error[L0514]: this argument is a copy, so changing it changes nothing
 --> src/main.rslisp:5:10
  |
1 | (defn bump! (x) (set! x (+ x 1)))
  |                  ---- `bump!` changes its parameter here
  |
5 |   (bump! (+ a 1))
  |          ^^^^^^^ this is a copy, not a place
  |
  = help: bind it to a local and pass the local: `(local value (+ a 1))`
```

The argument appears in the help as the user wrote it, and `value` is the placeholder that RFC 0058's helps use. The same report covers a field of a call's result, as `(:items (->box-of (vec 0)))`.

`L0514` carries the secondary label only when a `set!` of the whole parameter in the callee's body caused the mode. A callee with no such `set!` has no site to name, so it has no secondary label. `L0419` has no secondary label: `(push! (vec 0) 1)` reaches a `rust` form. When `field_place` finds a field of a value that can be a place, the existing help ``pass the field as a place: `...` `` replaces the local help, in both codes.

A top-level `def`, as `(bump! limit)`, emits `bump(&mut LIMIT)` on the prototype and changes a temporary. It is `L0514`, or `L0419` at a written parameter, with the help ``a `def` never changes: `(local value limit)` ``. A value that is not Copy reads `(local value (clone names))`, because a `def` holds one value and a `local` of it does not compile.

**An `at` element is a place a function can change.** `set!` already takes it as a place, and a `mut` argument now does too, after the build:

```lisp
(defn bump! (x) (set! x (+ x 1)))

(defn main ()
  (local scores (vec 1 2))
  (bump! (at scores 0))
  (println (at scores 0)))
```

```rust
fn main() {
    let mut scores: Vec<i64> = vec![1_i64, 2_i64];
    bump(&mut scores[0]);
    println!("{}", scores[0])
}
```

`(bump! (mut (at scores 0)))` emits the same, where `main` reports `L0419`, and the bare form is `L0401` on `main` at a written `(mut i64)`. The rule covers every `at` that `set!` covers: `(at (at grid y) x)` emits `&mut grid[y][x]`, `(at (:items order) 0)` emits `&mut order.items[0]`, and a map element emits `m.get_mut(&k).unwrap()`. A vector panics on an index out of bounds, and a map panics on a missing key, as under `set!`. The argument is a `mut` use of its collection, so `(defn age! (v) (bump! (at v 0)))` emits `fn age(v: &mut [i64])`, and a collection behind a shared borrow is `L0513`. A field of an `at` form stays no place, as it is under `set!`, and reports `L0514`. Two `mut` arguments of one collection are rustc's `E0499`, as two `mut` borrows of one local are today.

A shared borrow cannot change, so a field behind one, or a name of type `(ref a)`, gets a new code, `L0513`, `CHANGE_THROUGH_SHARED_BORROW`, at the ownership stage. It replaces rustc's `E0596` for a field, and for a name it replaces `bump(*y)`, which compiles on `main` and loses the write, and the bare `L0401` of a written parameter:

```text
error[L0513]: `(:year c)` cannot change, because `c` is a shared borrow
 --> src/main.rslisp:5:32
  |
5 | (defn f ((c (ref car))) (bump! (:year c)))
  |             ---------          ^^^^^^^^^ this field is behind a shared borrow
  |             |
  |             `c` is written as a shared borrow
  |
  = help: write `c` as `(mut car)`
```

A name reads `` `y` cannot change, because it is a shared borrow``, with the primary label `this name is a shared borrow`. The secondary label and the help depend on where the borrow comes from:

| The borrow | Example | Secondary label | Help |
| --- | --- | --- | --- |
| a written type | `(defn f ((y (ref i64))) (bump! y))` | `` `y` is written as a shared borrow `` | ``write `y` as `(mut i64)` `` |
| a closure parameter with no written type | `(map (iter v) (fn (y) (bump! y) 1))` | `` this call passes `y` as a shared borrow `` | ``this call fixes the type of `y`, so change a copy: `(local value y)` `` |
| a local | `(local c (ref k))` | `` `c` is bound to a shared borrow `` | ``bind it with `mut`: `(local c (mut k))` `` |
| `(ref self)` | `(bump! (:year self))` in a method | `` the method takes `self` as a shared borrow `` | ``write the receiver as `(mut self)` in the trait`` |

On a field, the closure case reads `(local value (:year c))`. A value that is not Copy reads `(local value (clone y))`.

A parameter with no written type never reports it, because the field read is a `mut` use of the struct: `(defn age! (c) (bump! (:year c)))` emits `fn age(c: &mut Car)`. A struct changed that way stops being Copy, by the existing rule under Struct traits.

### 6. The converse warning

**A top-level `defn` or `fn` whose name ends with `!` and whose body neither changes a parameter nor passes it on is `L0508`, `NEEDLESS_BANG`.** A written `mut` parameter counts only when the body uses it that way, so `(defn f! ((x (mut i64))) (println x))` fires. A trait method has no body to read, so it follows the same rule from its written signature, with a `mut` parameter or `(mut self)` as a change.

```text
warning[L0508]: `show!` changes no argument, so its name should not end with `!`
 --> src/main.rslisp:1:7
  |
1 | (defn show! (x) (println x))
  |       ^^^^^ this function changes no argument
  |
  = help: rename it `show`
```

A written by-value parameter that a `set!` changes adds the fix for that case:

```text
  = help: `x` is written by value, so `set!` changes a copy; to change the caller's value, write it as `(mut i64)`
  = help: rename it `bump`
```

A written `mut` parameter that nothing changes adds the other fix:

```text
  = help: nothing changes `x`, so it can be written as `i64`
  = help: rename it `f`
```

When the function is also `L0506` at a function value, by rule 7, the compiler drops `L0508` for that function, because both report one fault and renaming it would leave the error.

It does not fire on:

- **A change through a call.** `(defn g! (x) (bump! x))` infers `mut`, and a written `mut` passed to a `mut` parameter counts the same.
- **A function parameter.** `(defn tick! (f) (f) (f))` emits `impl FnMut()`, and its call changes what the closure captured. A parameter whose type is a function type counts as a change.
- **A `mut` inside a parameter's type,** as in `(vec (mut i64))` or a struct with a `(mut i64)` field.
- **An impl's method or a trait's default body,** because the trait's name governs them, as under `L0504`.
- **A value of an interior mutable type passed to a call,** by rule 9.
- **A local closure,** as `(local add! (fn (n) (set! total n)))`. `L0504` skips those too.
- **A `rust` form,** such as a prelude template. It has no body to read, and its signature states its modes.

It fires on a function that takes an owned value, changes it and returns it, as `(defn add! (v) (push! v 1) v)`: it emits `fn add(mut v: Vec<i64>)`, and no caller's value changes. It also fires on a `!` function with no parameters and on one that only prints, because rule 3 of Names covers arguments alone.

The repository has 145 `!` functions. The prototype, which has none of these exceptions and reads no body of a written `mut`, fires on one, `twice!` in `check/clone_meaning`, whose snapshot gains the warning. A probe of `tick!` fired on it too, which is why a function parameter counts as a change.

### 7. Decision 4 at the function value

**A function value whose by-value parameter meets an expected `(mut a)` is `L0506` when the function changes that parameter:** a `set!` of it, a `mut` use through a call, or a written `(mut x)`. After the build:

```text
error[L0506]: `bump!` changes its own copy of `x`, so the caller's value would not change
 --> src/main.rslisp:8:18
  |
2 | (defn bump! (x) (set! x (+ x 1)))
  |                  ---- this changes a copy
  |
8 |   (apply-change! bump! (mut total))
  |                  ^^^^^ this parameter expects `(mut i64)`, and `bump!` takes `i64`
  |
  = help: write the parameter as `(mut i64)`: `(type bump! (fn ((mut i64)) unit))`
```

The help prints the user's signature with that parameter changed, or `(x (mut i64))` for a written binder. A function that does not change the parameter keeps its wrapper, as a reader of the value:

```rust
apply_change(|argument: &mut i64| show(*argument), &mut total);
```

An expected `(ref a)` or a by-value parameter keeps its wrapper whatever the function does, because the expected type promises the caller no change: with a written `(type bump! (fn (i64) unit))`, passing `bump!` to a `(fn ((ref i64)) unit)` parameter still emits `|argument: &i64| bump(*argument)`.

An inferred `mut` parameter meets an expected `ref` as `L0507`, the existing report, because no adjustment fits it: `(defn twice (x) (set! x (* x 2)) (println x))` passed to `map` was a by-value function on `main`, and its fix is a written `(type twice (fn (i64) unit))` or a `local` copy. The existing help names the signature.

**An inferred `mut` parameter of a Copy type at an expected by-value parameter is an error, `L0515`, `CHANGING_FUNCTION_VALUE`.** The caller keeps its value, so the wrapper would lose the write. The prototype emits that wrapper, `|mut argument: i64| bump(&mut argument)`. It is its own diagnostic, at the ownership stage, after the build:

```text
error[L0515]: `bump!` changes its argument, but this call passes it a copy
 --> src/main.rslisp:6:15
  |
1 | (defn bump! (x) (set! x (+ x 1)))
  |                  ---- this would change only the copy
  |
6 |   (take-value bump! t)
  |               ^^^^^ this parameter expects `i64`, and `bump!` takes `(mut i64)`
  |
  = help: to change a copy on purpose, write `(type bump! (fn (i64) unit))`, or start the body with `(local x x)`
```

A value that is not Copy keeps the wrapper, as `|mut argument: Vec<i64>| grow(&mut argument)` on `main`, because the caller gave the value up, as under rule 2.

### 8. Other changes on `main`

**A closure that captures a parameter and sets it is a `mut` use of the parameter.** The prototype emits:

```lisp
(defn bump! (x)
  (local f (fn () (set! x (+ x 1))))
  (f)
  x)
```

```rust
fn bump(x: &mut i64) -> i64 {
    let mut f = || *x = *x + 1;
    f();
    *x
}
```

A returned closure does the same, so a parameter used as a closure's private state changes meaning:

```lisp
(defn counter (start) (fn () (set! start (+ start 1)) start))
```

```rust
fn counter(start: &mut i64) -> impl FnMut() -> i64 {
    move || {
        *start = *start + 1;
        *start
    }
}
```

`counter` now warns under `L0504`, `(counter 0)` is `L0514` after the build, and `(counter n)` changes `n` with each call of the closure. The fix is a local: `(local count start)` before the `fn`, which keeps today's `move` closure over its own value. No file in this repository holds such a closure.

A call that returns such a closure out of the function that owns the argument cannot compile. `(defn make () (local n 0) (counter n))` runs on `main`, and it is a bare rustc `E0597` on the prototype, as the `(vec 0)` form is on `main`. Both become `L0512` after the build, with a help of their own:

```text
error[L0512]: cannot pass a borrow of a local value to `counter`
 --> src/main.rslisp:2:36
  |
1 | (defn counter (start) (fn () (set! start (+ start 1)) start))
  |                ----- the closure that `counter` returns keeps this borrow
2 | (defn make () (local n 0) (counter n))
  |                                    ^ this local value is dropped when the function returns
  |
  = help: to give the closure a value of its own, start `counter` with `(local count start)`
```

The help reads `(local count (clone start))` for a value that is not Copy.

**A trait method's parameter keeps its written type,** because a trait states its signature and modes are not inferred for it. `(step (fn ((ref self) i64) i64))` with `(set! by (+ by 1))` in an impl stays `mut by: i64`, a local update. A default body's parameter follows the same rule, which `main` does not: its `set!` is rustc's `E0384`, because the parameter has no `mut`. After the build it emits `mut`:

```lisp
(trait stepper
  (step (fn ((ref self) i64) i64)
    (set! argument-1 (+ argument-1 1))
    argument-1))
```

```rust
trait Stepper {
    fn step(&self, mut argument_1: i64) -> i64 {
        argument_1 = argument_1 + 1;
        argument_1
    }
}
```

A method written `(mut i64)` gains rule 4, so `(step! (s by) (set! by (+ by 1)))` emits `*by = *by + 1`, where `main` reports `L0401`.

**A call that reads the value it changes keeps its order,** by the existing Call order rule: `(add-to! a a)` emits a `call_argument_1` copy before `add_to(&mut a, call_argument_1)`.

The change makes a program that ran behave differently, so its pull request takes the `release:minor` label.

### 9. Interior mutability

**A wrapped Rust type that changes through a shared borrow declares it with the marker `:interior-mutable`, the last form of its definition.** The name is the term Rust's documentation uses for the concept.

```lisp
(deftype (cell a) (rust "std::cell::Cell<{0}>") :interior-mutable)
```

`main` writes the form as `(rust-type (cell a) "std::cell::Cell<{0}>" :interior-mutable)`. The marker takes these rules:

- It appears at most once, after the `rust` clause and any type clauses, and only on a definition that names a Rust type. A marker anywhere else, a second marker or another keyword in its place is `L0337`, and a misspelled marker's help suggests the closest name.
- A type holds interior mutability when it is marked, or when a type argument, a struct field or an enum payload holds it, through any number of `ref` and `mut` layers. A function type holds none.
- It changes no emitted Rust, no mode and no trait. The compiler reads it from the type's definition, so no name such as `Cell` is restated in Rust.
- The prelude marks `stdin`, because `std::io::Stdin` reads through a shared borrow. It marks no other type.

**A body that passes a value holding interior mutability to a call, a Rust template or a receiver makes a possible change.** The compiler cannot see the change, so neither warning speaks:

- `L0508` treats the parameter as changed, so `(defn reset! (c) (cell-set c 0))` does not fire.
- `L0504` does not fire on a `mut` parameter of such a type whose only uses are such passes, so a plain name stays silent. A `set!` on the parameter is a certain change, and it warns as usual.

A `!` function that holds such a parameter and never passes it is `L0508`, because nothing it does can change the value.
