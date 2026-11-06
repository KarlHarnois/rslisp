# RFC 0057: One-line arms in the formatter

**Status:** Accepted
**Date:** 2026-11-05

## Problem

RFC 0053 and the sparse-list rule keep a list on one line when it is at most 40 columns long, or at most 60 columns long and holds at most two lists at any depth inside it. A `match` arm counts as any other list, and a variant pattern such as `(msg/increment fruit)` counts as one of the two lists. A short arm with a nested call then breaks, and the arms of one `match` mix layouts for no reason a reader can see:

```lisp
(defn update (msg model)
  (match msg
    ((msg/increment fruit)
     (no-effect (change model fruit 1)))
    ((msg/decrement fruit)
     (no-effect (change model fruit -1)))
    (msg/reset (no-effect (init)))
    (msg/load (pair model (load-apples)))
    ((msg/loaded result)
     (no-effect (apply-loaded model result)))))
```

The formatter names no form, so it needs a statement from the prelude to tell an arm from any other list.

## Options

### A `:arms` marker on the header shape

A form with arms states so on its `header-shape`, beside `:docstring` and `:one-per-line`:

```lisp
(header-shape (match value) :arms)
(header-shape (cond) :arms)
(header-shape (defrule name) :docstring :arms)
```

Each list after the header, other than a keyword's value, is an arm. A user macro with arms opts in with the same line.

### Count lists differently in every list

The sparse rule stops counting the pattern, or raises its limit, for every list. Every call then changes layout, not only arms, and the rule still reads a pattern as a list.

### Name the forms in the formatter

A Rust table lists `match`, `cond` and `defrule`. A user macro cannot opt in, and the principles forbid restating a prelude fact in Rust.

## Recommendation

The `:arms` marker. An arm stays on one line when it is at most 60 columns long, the fill width, and fits in the line width, whatever the number of lists inside it. An arm over 60 columns breaks as it does under RFC 0053. Each arm decides alone, so one long arm does not break the short ones:

```lisp
(defn update (msg model)
  (match msg
    ((msg/increment fruit) (no-effect (change model fruit 1)))
    ((msg/decrement fruit) (no-effect (change model fruit -1)))
    (msg/reset (no-effect (init)))
    (msg/load (pair model (load-apples)))
    ((msg/loaded result)
     (no-effect (apply-loaded model result)))))
```

The last arm is 61 columns, so it breaks.

## Decision

The project owner decided on 2026-11-05 the rule and the marker above. The rule covers `match`, `cond` and `defrule`. The prelude has no `case`. `:arms` follows the rules of `:docstring` and `:one-per-line`: at most once, any other marker is `L0331`, and an unknown marker's help suggests the closest one.

## Rejected options and why

- **Count lists differently in every list:** it moves calls that are not arms, and leaves the mixed layouts of arms over the limit it picks.
- **Name the forms in the formatter:** a user macro with arms could not opt in, and the Rust would restate what the prelude states.

## Rule

[docs/language.md: Source formatting](../docs/language.md#source-formatting) and [docs/language.md: Header shapes](../docs/language.md#header-shapes)
