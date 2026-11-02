# RFC 0046: Names

**Status:** Accepted
**Date:** 2026-11-02

## Problem

The prelude names its functions without a rule, so a user cannot predict a name from its meaning:

- Some concepts have two names. `str` and `to-string` both turn a value into a `string`. `slurp` and `slurp-stdin` both read all of an input. `len` counts a collection in a program, and the `code` module's `length` counts a list in a macro.
- `values` yields copies of a vector's elements, and `vals` yields a map's values. The two names read as one word and mean different things.
- `any` and `all` answer yes or no, but their names lack the `?` that every other predicate ends with.

## Options

### Rank source languages

Take a name from Clojure first, then the most widely known name in another language. `fold` stays, because Clojure's `reduce` without an initial value differs, and `any` becomes `some`, as in Clojure.

```lisp
(fold (iter years) 0 add)
(some (iter years) recent?)
```

```rust
years.iter().fold(0, |a, b| a + b)
years.iter().any(is_recent)
```

### Five rules

Each name follows five rules:

1. A name is kebab-case.
2. A predicate ends with `?`.
3. A function that changes an argument ends with `!`. A `!` name next to a plain name is the plain name's in-place form, as `reverse!` is to `reverse`.
4. A prelude type constructs by its own name, as `vec` and `pair` do. A `defstruct` type constructs by `->name`, as `->car` does (RFC 0038).
5. One concept has one name.

```lisp
(reduce (iter years) 0 add)
(any? (iter years) recent?)
```

```rust
years.iter().fold(0, |a, b| a + b)
years.iter().any(is_recent)
```

## Recommendation

Five rules, with each exception listed. A reader who knows the rules can predict a name from its meaning, and a reviewer can check a new name against them.

## Decision

The project owner accepted the five rules and these changes. Each change breaks programs that use the old name.

| Now | After | Reason |
|---|---|---|
| `fold` | `reduce` | the name most programmers know |
| `lookup` | `get` | the usual name for reading a map; one read for maps and slices |
| `then-some`, `then-ok` | `then` | one name for chaining a step that can fail; see #280 |
| `values` | `copied` | says what it yields; ends the clash with `vals` |
| `box-new` | `box` | rule 4 |
| `to-string` | `str` | rule 5 |
| `any` | `any?` | rule 2 |
| `all` | `every?` | rule 2 |
| `sort-by-key!` | `sort-by!` | shorter, same meaning |
| `sort-descending!` | `(sort! v >)` | one `sort!` for every order |
| `!=` | `not=` | reads as `not` and `=` |
| `append!`, `append-char!` | `push!` | rule 5 |
| `length` (`code` module) | `len` | rule 5 |
| `entries` | `iter` | rule 5 |
| `slurp-stdin` | `(slurp stdin)` | rule 5; see RFC 0045 |

`then` and `(slurp stdin)` are not built here. `then` needs type-checker changes that issue #280 tracks, and RFC 0045 builds `(slurp stdin)`.

These names break a rule, and the owner accepted each:

- `str` builds a `string`, an exception to rule 4. The owner kept the short name that every program writes.
- `then-some` and `then-ok` break rule 5 until #280 lands.

These names stay, deliberately: `collect`, `len`, `defstruct`, `vals`, `at`, `push!` on vectors, `max-by-key`, `dedup!`, `lines`, `for-each`, `iter`, `find` and `remove-at!`. `rem`, `add` and the other arithmetic names stay exported, because `rem` is the only remainder operator and a program passes `add` as a value.

The owner chose these meanings:

- `reduce` has two arities. `(reduce xs f)` emits `xs.reduce(f)` and returns an `option`. `(reduce xs init f)` emits `xs.fold(init, f)`.
- `then` takes an `option` or a `result` and emits `.and_then(...)`. It needs a trait over the two types.
- `get` reads a slice by index or a map by key.
- `push!` adds an element to a vector, and a `(ref str)` or a `char` to a string.
- `str` with one argument emits `x.to_string()`, and `str` as a value is the one-argument function, so `(map xs str)` works as `(map xs to-string)` did. The call's type stays `(where ((a display)) (fn ((ref a)) string))`.
- `sort!` takes an optional order, so `(sort! v >)` sorts in descending order.
- `find`, `any?` and `every?` consume their iterator, as `count` and `reduce` do. A binding emits `values.into_iter().find(...)`, and a temporary such as `(iter years)` emits `years.iter().find(...)`. No binding becomes `mut` for them, and a reused iterator is a use after move.
- `iter` is the one name for walking a map.

This build chose two designs:

- `get` extends the `map-at` kernel form. `map-at get` accepts a vector or a slice beside a map, as `map-at iter` already does, and emits `.get(index)` for a slice. A trait would need an index type that depends on the collection, and each call on an unknown collection would then make a function generic over that trait. One `map-at` operation adds no trait and no runtime part.
- `push!` extends `map-at` too, with a `push` operation. A Rust trait method would need one name for `Vec::push`, `String::push` and `String::push_str`, and Rust resolves `push` on a `String` to the inherent method that takes a `char`, so a `(ref str)` argument would not compile. `map-at push` picks the method from the inferred types, and the emitted Rust stays `.push(x)` and `.push_str(text)`.

## Rejected options and why

- **Rank source languages:** the owner chose names for their own reasons, such as `reduce` over `fold` and `any?` over `some`, so every choice became an exception to the ranking.

## Rule

[docs/language.md: Names](../docs/language.md#names)
