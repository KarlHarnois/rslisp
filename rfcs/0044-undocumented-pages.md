# RFC 0044: Pages for undocumented definitions

**Status:** Accepted
**Date:** 2026-10-31

## Problem

`(doc term)` and `rslisp doc` report `L0327` for a name that exists but has no docstring, so `rslisp doc defrule` fails where `go doc` prints what it knows. The page of a documented definition starts with its header as written, which the definer stores beside the docstring in its module's notes (RFC 0042). An undocumented definition stores nothing, so a lookup has no header to show. Storing a header for every definition makes every program pay for each definition at expansion time, whether or not anyone looks it up.

## Options

Every option keeps `(docstring term)` at `L0327` for a term without a docstring and an unknown term at `L0201`. Each cost below is a callgrind total of a release `rslisp expand`, the median of 5 to 11 runs, on `main` at `09c44e47`. A cost per definition is the extra cost of one undocumented `defn`, with the empty program subtracted, from 50 and 200 undocumented `defn`s.

### A: `L0327` as today

The lookup fails for a term without a docstring, and `rslisp doc` exits 1.

```lisp
(defn twice (x) (* 2 x))
(def twice-page (doc twice))
```

```text
error[L0327]: `twice` has no docstring
```

### B: a name-only page

Every term without a docstring gets its bare name, a blank line, then `No documentation.`. The lookup needs no new built-in, and no definition stores anything. The page drops the header that a documented page shows, such as the parameter list.

```lisp
(defn twice (x) (* 2 x))
(def twice-page (doc twice))
```

```rust
const TWICE_PAGE: &str = "twice\n\nNo documentation.";
```

### C: a header note per definition

Each definer notes `("value" twice (defn twice (x)))` for an undocumented definition, the way it notes the header and the docstring of a documented one. The lookup reads the note and builds the text. Prototype `research-undocumented-headers-prototype` (`330041c5`) measured these costs:

- 21.0K per undocumented `defn` at 50, and 21.2K on the 200 slope.
- 18.5K when each definer builds the note inside its branches.
- +1.69M for the empty program, because the prelude's own 51 definitions note their headers too.

```lisp
(defn twice (x) (* 2 x))
(def twice-page (doc twice))
```

```rust
const TWICE_PAGE: &str = "(defn twice (x))\n\nNo documentation.";
```

### D: the header rebuilt at lookup

A definition stores nothing. A new `code` built-in, `(definition-call term)`, returns the macro call, as written in the source, that defined the binding `term` refers to. It reads the call sites that the expander already records in `macro_origins`, and re-reads the call from the source map. Each definer states its header once, as in `(header-shape (defn name parameters))`. The lookup keeps as many leading forms of the call as the shape lists. A name that no macro call defined, or whose macro states no shape, shows its bare name.

Prototype `research-undocumented-headers-lookup` (`089790da`) measured these costs:

- +0.1K per undocumented `defn` at 50, and -0.1K on the 200 slope.
- +1.51M for the empty program.
- 0.26M to 1.25M for one lookup, most of it spent re-reading the defining file.

```lisp
(defn twice (x) (* 2 x))
(def twice-page (doc twice))
```

```rust
const TWICE_PAGE: &str = "(defn twice (x))\n\nNo documentation.";
```

## Recommendation

Option D. It gives the page that D9 asks for and costs nothing per definition. Its empty-program cost comes from the larger lookup code and the four `header-shape` calls. A cheaper `header-shape` and a reader entry point that reads one form are follow-ups.

## Decision

The project owner decided on 2026-10-31.

- **D9:** "`doc` gives a page for a name that exists but has no docstring, as `go doc` does. [...] The page of an undocumented definition is its header, a blank line, then `No documentation.`, and `rslisp doc` exits 0. A bound name with no header note gets the same page with the bare name as header. `(docstring term)` keeps `L0327` when there is no docstring. An unknown name keeps `L0201`."
- **D9b:** "header notes per definition cost 18.5K to 21K, over D9's 15K ceiling, so research a near-zero-cost way to rebuild the header at lookup from the call that created the binding, through one new general `code` built-in."
- **D10:** "accept two pieces of language surface for D9. (1) `(definition-call term)`, a public general `code` built-in: it returns a list holding the macro call, as written in the source, that defined the module-level binding `term` refers to (the innermost recorded call around the name), or an empty list for a binding no macro call defined (a plain `def`, a local, a kernel form). An unbound term is `L0201`. (2) `(header-shape (definer name parameters))`, which lets a definer state once how its header looks. The four prelude definers state theirs, and a user definer may state its own. `doc` rebuilds an undocumented definition's header at lookup from these, so a definition stores nothing."

The build adds `definition-call` and `header-shape`, and `defn`, `defmacro`, `defstruct` and `defrule` state their shapes. The owner also accepted three behaviors:

- The empty program costs about 1.5M more.
- A struct's constructor shows the `defstruct` header.
- `(header term)` returns the bare name when no header is written.

## Rejected options and why

- **A: `L0327` as today:** rejected by D9, because a name that exists gets a page.
- **B: a name-only page:** rejected by D9, which makes the page of an undocumented definition its header. Option D keeps the bare name only for a name that no definer made.
- **C: a header note per definition:** rejected by D9b, because 18.5K to 21K per definition is over D9's ceiling of 15K.

## Rule

[docs/language.md: Docstrings](../docs/language.md#docstrings)
