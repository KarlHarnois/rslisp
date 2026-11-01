# RFC 0043: A written deref leaves a borrow to the rule

**Status:** Accepted
**Date:** 2026-10-31

## Problem

`filter`, `find`, `take-while` and `max-by-key` pass each item to their test as a reference, as Rust does. Over `(iter years)` the item is a `(ref i64)`, so the test's parameter is a `(ref (ref i64))`. The everyday pipeline below writes one `deref`, which leaves a `(ref i64)`.

```lisp
(count (filter (iter years) (fn (y) (> (deref y) 2020))))
```

D1 turned the borrow rule off for every written `deref`, so the compiler did not copy the year out of the borrow that remained. The type stage borrowed the literal instead, the emitter dropped that borrow, and rustc rejected the emitted Rust:

```rust
years.iter().filter(|y: &&i64| *y > 2020).count()
```

```text
error[E0308]: mismatched types
  |
3 |   (println (count (filter (iter years) (fn (y) (> (deref y) 2020))))))
  |                                                             ^^^^ expected `&i64`, found integer
  |
  = note: reported by rustc while compiling the generated Rust
  = help: consider dereferencing the borrow
```

`map`, `any`, `all`, `fold` and `for-each` pass the item itself, so the same closure ran there. To write a working test, a user had to know which Rust adapter adds a borrow.

## Options

### A: a written deref removes one borrow, and the rule meets the rest

A written `deref` removes exactly one borrow. A result that is still a borrow meets its expected type by the usual rule: the compiler adds one borrow, removes borrows, or copies a Copy value of 16 bytes or less out of a borrow, and never clones. A written `deref` whose result is not a borrow stays exact, and so does every written `ref` or `mut`.

```lisp
(count (filter (iter years) (fn (y) (> (deref y) 2020))))
```

```rust
years.iter().filter(|y: &&i64| **y > 2020).count()
```

### B: keep D1, and report that the deref is not needed

A written `deref` stays exact. A `deref` whose result is still a borrow where the rule would adjust it is an error, with help that says to drop the `deref`, because `(> y 2020)` already meets the rule.

```lisp
(count (filter (iter years) (fn (y) (> y 2020))))
```

```rust
years.iter().filter(|y: &&i64| **y > 2020).count()
```

### C: a written deref reads through every borrow

`deref` removes every borrow, so `(deref y)` on a `(ref (ref i64))` is an `i64`.

```lisp
(count (filter (iter years) (fn (y) (> (deref y) 2020))))
```

```rust
years.iter().filter(|y: &&i64| **y > 2020).count()
```

## Recommendation

Option A. The pipeline runs as written in every adapter, the rule that already meets every other value meets the borrow `deref` leaves, and `deref` keeps one meaning: it removes one borrow.

## Decision

The project owner decided K38: "a written `deref` removes exactly one borrow; any borrow that remains meets the expected type by the usual rule of `docs/language.md` 8.1 (add one borrow, remove borrows, or copy a small Copy value out; never clone). A written `ref` or `mut` still turns the rule off for its value."

The owner chose A because it keeps ordinary code free of Rust details, and keeps `deref` a precise one-step operation while `ref` and `mut` stay exact. This RFC supersedes the written-`deref` clause of D1.

One program that ran before emits different Rust with the same result: `(> (deref z) (deref z))` with `z` a `(ref (ref i64))` emitted `*z > *z`, which compares the two borrows, and now emits `**z > **z`.

## Rejected options and why

- **B: keep D1, and report that the deref is not needed:** rejected by K38. The user still has to know which Rust adapter adds a borrow, and a `deref` that runs in `map` becomes an error in `filter`.
- **C: a written deref reads through every borrow:** rejected by K38. `deref` would no longer be one step, so a program could not take a `(ref a)` out of a `(ref (ref a))` with it, and reading a value that is not Copy through every borrow would move it out of a borrow.

## Rule

[docs/language.md: 8.1 References at call sites](../docs/language.md#81-references-at-call-sites)
