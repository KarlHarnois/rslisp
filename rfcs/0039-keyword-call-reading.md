# RFC 0039: Reading keyword pairs in a call

**Status:** Accepted
**Date:** 2026-10-27

## Problem

RFC 0038 reads a call of `->car` or of a keyword function in this order: a valid call by name, else a call with one argument per field, else an error. A call by name that leaves out fields can keep exactly one argument per field, so it silently reads as positional, and its error talks about types instead of names:

```lisp
(defstruct car :make str :year u16)
(->car :make "Nissan")   ; forgot :year
```

```text
error[L0401]: mismatched types
  expected `str`, found `(fn (car) (ref str))`
```

A misspelled key does the same: `(->car :mak "Nissan")` reports `L0407` "cannot tell which struct's `mak` this is". The same mistake on a struct with a different field count gives the right error, so the message depends on how many fields the struct has.

A positional call can also pass a field-read keyword as a value, where a field holds a function:

```lisp
(defstruct sorter :key (fn ((ref user)) (ref str)) :label str)
(->sorter :name "by name")
```

## Options

### A. Keep the order of RFC 0038

`(->sorter :name "by name")` works. `(->car :make "Nissan")` and `(->car :mak "Nissan")` keep their type errors.

### B. Keyword pairs always mean a call by name

`(->car :make "Nissan")` reports the missing field `year`, and `(->car :mak "Nissan")` suggests `:make`. `(->sorter :name "by name")` fails and must be written `(->sorter :key :name :label "by name")`.

### C. Keys decide

A call is by name if any key names a field, and positional otherwise. `(->car :make "Nissan")` and `(->sorter :name "by name")` work, but `(->car :mak "Nissan")` keeps the `L0407` error.

### D. Declared field types decide

A keyword argument reads as a value only where it lands on a field declared as a function type. Otherwise keyword pairs are a call by name.

```lisp
(->car :make "Nissan")      ; error: missing field `year` in `car`
(->car :mak "Nissan")       ; error: `car` has no field `:mak`, help: `:make`
(->sorter :name "by name")  ; positional: `:name` lands on `key`, a function
```

```rust
let by_name = make_sorter(|value: &User<'_>| value.name, "by name");
```

## Recommendation

D. It is the only option that reports every mistake in a call by name with an error about names and still lets a positional call pass a field-read keyword to a function field.

## Decision

The project owner chose D on 2026-10-27 (decision K17). A call of `->name` or of a keyword function reads its arguments in this order:

1. Keyword pairs that make a valid call by name are a call by name.
2. Otherwise, keyword pairs with one argument per field are positional when every keyword at an even position lands on a field whose declared type is written as a function type.
3. Otherwise, keyword pairs are a call by name, and the error names the missing, unknown or repeated field.
4. Arguments that are not keyword pairs keep RFC 0038's rule.

A type parameter and a keyword parameter with no written type do not count as a function type. Keyword parameters follow the same rule, with parameters in place of fields.

## Rejected options and why

- **A:** the error for a forgotten or misspelled key depends on the struct's field count.
- **B:** a positional call cannot pass a field-read keyword to a function field.
- **C:** a misspelled key still reads as positional and reports a type error.

## Rule

[docs/language.md: Structs](../docs/language.md#structs), [docs/language.md: Keyword parameters](../docs/language.md#keyword-parameters)
