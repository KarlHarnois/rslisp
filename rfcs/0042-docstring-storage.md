# RFC 0042: Docstring storage in module notes

**Status:** Accepted
**Date:** 2026-10-31

## Problem

RFC 0041 stores each docstring as a hidden `doc/<name>` or `doc-type/<name>` macro beside its definition. The expander registers, lowers and checks every such macro, so a documented definition costs far more to compile than an undocumented one. On callgrind runs of `rslisp expand`, 50 documented `defn`s cost 183K instructions each more than 50 undocumented ones. The prelude is about to document roughly 100 terms, and every user program pays the same cost for its own documented definitions. The hidden binding also takes a name: a user `(def doc/twice 1)` beside a documented `twice` is `L0203`.

The prelude cannot gather a module's docstrings into one binding by itself. Each definer expands on its own, macro code keeps no state between calls, and a second `def` of one name is a duplicate.

## Options

Every option keeps what a user writes and what the lookups return. Each cost below is the median extra cost of one documented `defn`, from 50 documented against 50 undocumented `defn`s, over 5 callgrind runs.

### A: a hidden macro per definition (RFC 0041)

Each definer adds `(def doc/twice (macro (doc-form) (doc-part (nth doc-form 1) (quote (defn twice (x))) "Doubles a number.")))`, and a lookup expands through it. Cost: 183K per definition.

```lisp
(defn twice (x) "Doubles a number." (* 2 x))
(def twice-doc (docstring twice))
```

```rust
const TWICE_DOC: &str = "Doubles a number.";
```

### B: a plain value per definition

Each definer adds `(def doc/twice (quote ((defn twice (x)) "Doubles a number.")))`, and a new `code` built-in, `global-value`, reads the value at expansion time. The printed core must hide a macro-written quoted `def`, or every documented definition shows in `rslisp expand`. Prototype `research-doc-storage-value` (`dca76fd0`). Cost: 138K per definition. The source and the emitted Rust are those of option A.

### C: one table of notes per module

Each definer adds the note `("value" "twice" (defn twice (x)) "Doubles a number.")` under the key `docstrings` of its module's table with `add-module-note`, and defines no binding. A lookup reads the table of the module that defines the term with `module-notes`. Prototype `research-doc-storage-table` (`981d62ef`). Cost: 95K per definition in the prototype, and 93K in the build. The source and the emitted Rust are those of option A.

## Recommendation

Option C. It cuts the cost of a docstring by about half, adds two general built-ins and no kernel form, and frees the `doc/<name>` names. Elixir's `@doc` and Julia's `META` store docstrings in one table per module the same way. The rest of the cost is the definer's own work on one more body form, which every option shares.

## Decision

The project owner decided on 2026-10-31.

- **D7:** "docstrings are stored in one table per module, as Elixir's `@doc` and Julia's `META` do, instead of one hidden `doc/<name>` macro per definition. The definers add a note to their module's table, and `doc`, `docstring` and `header` read the table of the module that defines the term. This adds two general `code` built-ins over a per-module table in the expander (prototype `research-doc-storage-table`, 981d62ef: `add-module-note` and `module-notes`). A note added during a trial expansion that fails is rolled back. The saved prelude state includes the table. A user `(def doc/name ...)` no longer collides. What users write and what the lookups return stay the same."

The build rolls a note back whenever the macro call that added it fails, and whenever the expander discards a trial expansion, so a note stays only with an expansion the expander keeps. The lookups no longer test for a binding, so the build removes `bound?`, which only those lookups used.

## Rejected options and why

- **A: a hidden macro per definition:** rejected by D7. Each entry is a macro that the expander lowers and checks, which costs about 77K of the 183K, and its name collides with a user's `doc/<name>`.
- **B: a plain value per definition:** rejected by D7. It costs 138K per definition, keeps the name collision, and needs the printed core to hide a kind of `def`.

## Rule

[docs/language.md: Docstrings](../docs/language.md#docstrings)
