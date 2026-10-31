# RFC 0041: Docstrings and the doc lookups

**Status:** Accepted
**Date:** 2026-10-30

## Problem

The language has no way to document a definition, and no way to read documentation back. The project owner's plan asks for three front ends that share one lookup:

- `(doc <term>)`, called from the language itself, as in Clojure.
- `rslisp doc <term>`, a thin wrapper over the same lookup, for prelude terms and for the user's own program.
- A future REPL that reuses the same lookup.

The design must keep the reader's special characters, add no kernel form the prelude can avoid, emit nothing at run time, and give a formatter a syntax it can lay out. Before this RFC, `(defn twice (x) "Doubles a number." (* 2 x))` kept the string as a discarded body value, and `(doc twice)` was `L0201`, because no `doc` existed.

## Options

The position of the docstring is decided (see Decision). The options below are about where a docstring is stored and how a lookup reaches it.

### A: an exported `doc/<name>` macro

Each definer stores its docstring as a macro `doc/<name>` beside the definition. `(doc name)` expands through `doc/name`, built with `symbol` in the scope where the user wrote `name`. A module that exports `area` must also export `doc/area`, and an importer must write `(use geometry area doc/area)`.

```lisp
(defn twice (x) "Doubles a number." (* 2 x))
(def twice-doc (docstring twice))
```

```rust
const TWICE_DOC: &str = "Doubles a number.";
```

### B: a `doc/<name>` macro found in the defining module

The storage of A. The lookups build the companion name with `definition-symbol`, a new `code` built-in that takes the scopes of the module that defines the binding `name` refers to. The companion then resolves beside the definition, the way a macro's private helpers resolve. `(use geometry area)` is enough, and the prelude exports no `doc/...` names. `bound?`, a second new built-in, lets the lookups report a name without a docstring.

```lisp
(use geometry area)
(def area-doc (docstring area))
```

```rust
const AREA_DOC: &str = "Multiplies a width by a height.";
```

### C: docstrings as binding metadata in the expander

`def` takes an optional docstring, `(def name "doc" value)`. The expander stores it in `BindingInfo`, and a built-in `(docstring name)` reads it. The emitted Rust is the same as in B.

```lisp
(def twice "Doubles a number." (fn (x) (* 2 x)))
```

## Recommendation

Option B.

- It keeps "If the prelude can express a feature, NEVER add a kernel form for it": storage and lookup are prelude macros. The compiler gains three built-ins that state no prelude fact. `definition-symbol` is the module-scoped twin of `symbol`, and `bound?` is a resolution test.
- It keeps "NEVER emit a clone or an allocation that the source does not write": a lookup is a string literal, and a `doc/<name>` macro emits no Rust.
- It reaches imported and re-exported terms through the binding itself, which A cannot do without a second export for every documented name.

## Decision

The project owner decided on 2026-10-30.

- **D1:** "A docstring goes after the parameter list, as the first body form, and counts as a docstring only when another form follows it: `(defn greetings (name) "Greets." (str "hi " name))`. `(defn greeting () "hi")` returns `"hi"`. Definers without a parameter list take the docstring as the first form after their header, such as `(defstruct car "A car." ...)`." The header line looks the same with or without a docstring, so a formatter lays both out alike. The accepted costs: the follows-a-form rule, a forgotten body that returns the docstring, and no multi-arity `defn` in this position.
- **D2:** "`(doc term)` finds the docstring in the module that defines `term`: two general `code` built-ins, `definition-symbol` (a symbol in the scope of the module that defines a binding) and `bound?`, let the prelude `doc` build `doc/<term>` there. `(use geometry area)` is enough. No `doc/<name>` export or import. Storage stays a prelude `doc/<name>` macro beside each definition." This is option B. The build adds a third built-in, `type-definition-symbol`, so a struct's `doc-type/<name>` entry resolves in the module that defines the type, even when a value of the same name comes from another module.
- **D3:** "`defn`, `defmacro`, `defstruct` and `defrule` take a docstring in the D1 position. Kernel forms are documented from the prelude by a prelude-internal helper, such as `(document if "Chooses a branch: (if test then else).")`. The helper is not exported to users. A user documents through the definers only." The helper comes in a later build. `definition-symbol` keeps the scopes of the term for a kernel name, so the helper can store `doc/if` beside the prelude.
- **D4 and D4b:** the page is the definer's header as written, flush left, a blank line, then the docstring. It shows no module, no file and line, and no import hint. The inferred type is a later flag.
- **D5:** "three prelude lookups. `(doc term)` returns the page as a string: the header, a blank line, the docstring. `(docstring term)` returns the docstring text. `(header term)` returns the definition's first line as written, such as `(defn area (width height))`." The CLI command, a later build, takes the one Rust name `doc`.
- **D6:** "docstrings are Markdown. `docstring` (and so `doc`, the CLI and the REPL) strips the indentation that all continuation lines share, at expansion time in the prelude (Python `inspect.cleandoc` rule). `docs/language.md` states the content rules: the first line is one complete summary sentence. Lines stay within 80 columns. Continuation lines are indented to the body. Markdown conventions apply. No compile error enforces width."

## Rejected options and why

- **Docstring after the name, `(defn twice "Doubles a number." (x) (* 2 x))`:** rejected by D1. Its header line changes shape with a docstring, so a formatter would lay out a documented and an undocumented `defn` differently. The form is `L0328`.
- **A: exported `doc/<name>`:** rejected by D2. Every documented name needs a second export and a second import, and `(doc area)` fails with `L0201` for an imported `area` unless the importer also imports `doc/area`.
- **C: binding metadata:** rejected by D2. It changes the kernel form `def` and adds expander state, but the prelude can express storage. "If the prelude can express a feature, NEVER add a kernel form for it."
- **A page with the module, the file and line, or an import hint:** rejected by D4. The page shows what the user wrote and nothing the lookup infers.
- **One lookup with a flag that picks the page, the text or the header:** rejected by D5 and by "NEVER write a flag argument". The three lookups are three names.

## Rule

[docs/language.md: Docstrings](../docs/language.md#docstrings)
