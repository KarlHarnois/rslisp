# RFC 0040: Unsized rust-type parameters

**Status:** Accepted
**Date:** 2026-10-28

## Problem

RFC 0038 makes a view stored in a struct field or an enum payload a borrow. PR 154 extended that rule to a view nested in a field's type argument, so `(vec str)` emits `Vec<&'a str>` and `(vec "Leaf" "Ariya")` fills it with no allocation. Applied without exception, the rule also rewrites `(box str)` to `Box<&'a str>`, but Rust's `Box` accepts an unsized argument and the source wrote `(box str)`, so `Box<str>` is the meaning the source has today.

A Rust type takes an unsized argument only behind a pointer. A `rust-type` could not say which of its parameters allows that, so the compiler could not tell `box` from any other Rust type without naming it in Rust, and a program's own `Rc` or `Arc` type would keep the wrong view.

## Options

### A. Mark the parameter in the head

A `rust-type` head writes a parameter that may be unsized as `(name unsized)`:

```lisp
(rust-type (box (item unsized)) "Box<{0}>")
(rust-type (shared (item unsized)) "std::rc::Rc<{0}>")
```

```rust
Box<str>
std::rc::Rc<str>
```

A view nested in a field's type argument becomes a borrow, except as the direct argument of a marked parameter. The mark reaches only that argument, so `(box (vec str))` keeps `Box<Vec<&'a str>>` because the vector inside the box still takes the borrow rule.

### B. Borrow only inside types that are not pointers

The compiler borrows a nested view only inside a type whose Rust template does not start with a smart pointer such as `Box`, `Rc` or `Arc`.

### C. Never borrow a view in a type argument

A `(vec str)` field stays an error, with a help line that says to write `(vec (ref str))`, which already works. Only a bare `str` field borrows, as RFC 0038 says.

## Recommendation

A. The mark sits beside the parameter it changes, so a reader sees which argument may be unsized. The prelude states that `box` marks `item`, and a program states the same for its own type, so the compiler names no Rust type. The rule stays one rule: a view in a type argument borrows, with an exception written in the same head.

## Decision

The project owner chose A on 2026-10-28 (decision K19). A `rust-type` head marks a parameter that may be unsized as `(name unsized)`. The prelude marks `box`, and a user's own `Rc` or `Arc` type marks its parameter the same way. A view nested in a field's type argument becomes a borrow except as the direct argument of a marked parameter.

## Rejected options and why

- **B:** the compiler would read Rust pointer names out of templates, which restates Rust facts in Rust code, and it would miss a program's own pointer types.
- **C:** `(vec "Leaf" "Ariya")` would keep its type error, and a nested view would follow a different rule from a bare one.

## Rule

[RFC 0038: Field keywords](0038-field-keywords.md), [docs/language.md: Types](../docs/language.md#types), [docs/language.md: Rust interop](../docs/language.md#rust-interop)
