# RFC 0051: A redefinition at the prompt takes effect

**Status:** Accepted
**Date:** 2026-11-04

## Problem

RFC 0047 rule 6 checks the prompt's buffer after each definition input, and rejects the input if the check fails, "so the buffer never holds a definition that does not check". A redefinition that breaks an earlier input is therefore rejected. This session runs at the prompt of pull request 299:

```text
> (defstruct user :name str)
> (def karl (->user :name "karl"))
> (defstruct user :name str :email str)
error[L0409]: missing field `email` in `user`
 --> <input 2>:1:11
  |
1 | (def karl (->user :name "karl"))
  |           ^^^^^^^^^^^^^^^^^^^^^ each missing field needs a value
```

The user redefined `user`, and the prompt kept the old `user`. The error points at `karl`, an input the user did not just type, and nothing says that the `defstruct` was rejected. To change `user`, the user must first find and redefine every input that uses it, in an order that checks at each step. The buffer is invisible, so the user cannot see which inputs those are.

The rule makes the most recent input the weakest one, where the user means it to be the strongest. Clojure, GHCi, IEx, Julia and Jupyter all let a redefinition take effect.

Rule 6 also rejects a replacement that would drop a name with `L0203`, as a `defn` of `->point` that replaces a `defstruct point`, or `(use shapes circle)` after `(use shapes circle square)`. That is the same problem: the earlier input wins over the one the user just typed.

## Options

### A: reject the input, as today

Rule 6 as it stands. A variant adds a note to each error, "`<input 3>` was rejected because `karl` no longer checks against it", but the user must still redefine `karl` before `user`.

### B: the new input takes effect, and each earlier input that no longer checks is set aside

The new input joins the buffer as RFC 0047 rule 6 places it. An earlier input that no longer checks leaves the buffer, and the prompt prints one warning for it. The rules below define it.

### C: keep each earlier input checked against the definitions it was entered with

GHCi and Clojure keep the old type alive beside the new one, so `karl` stays a value of the old `user`. The prompt has no state but the text of the buffer, and each expression compiles the whole buffer again, so a module would need two types named `user`. That needs versioned names in the expander, the checker and the emitter.

### D: set aside every input that names a redefined name

Instead of checking, the prompt sets aside each input that refers to a name the new input defines, and each input that refers to a name set aside. It sets aside inputs that still check, such as `(defn name-of (u) (:name u))` after a `user` that only gains a field.

## Recommendation

Option B, with one location rule, one loop and one warning.

The rounds that set inputs aside, and the notes for inputs that do not come back, are costs that RFC 0047 counted against a stateful session. They are accepted here because the owner ranks a redefinition that takes effect above them, and the buffer stays text: nothing outlives the prompt, and each expression still compiles the whole buffer.

### 1. The location of an error

**An error whose primary label, or one of whose `in this macro call` labels, lies in the new input is the new input's.** Otherwise it is located at the first input, in this order, that holds:

1. its outermost `in this macro call` label,
2. its primary label,
3. a `use` that reaches the file of its primary label, the earliest such input.

**An error located at no input is the new input's,** as one in the prelude. Secondary labels never decide. With the `pick` and `mk` of rule 4 in the buffer, a new `(do (defn pick (a b) b) (defn bar () 9))` makes `(mk foo bar)` bind `bar` too, and the `L0203` at `(mk foo bar)` sets that input aside, although its `first defined here` label lies in the new input.

Neither label wins alone. The new input owns every error it takes part in, whether it calls a broken macro or defines one:

```text
> (defmacro bad () (template (missing-helper 1)))
> (defn f () (bad))
error[L0201]: cannot find `missing-helper` in this scope
 --> <input 1>:1:29
  |
1 | (defmacro bad () (template (missing-helper 1)))
  |                             ^^^^^^^^^^^^^^ not found in this scope
  |
 ::: <input 2>:1:12
  |
1 | (defn f () (bad))
  |            ----- in this macro call
  |
  = note: in this expansion of `bad`
```

The call label lies in `<input 2>`, the new input, so the prompt rejects `f` and keeps `bad`, as today. In the reverse case the new input defines the broken macro, and an earlier input calls it:

```text
> (defmacro bad () (template 1))
> (defn f () (bad))
> (defmacro bad () (template (missing-helpr 1)))
error[L0201]: cannot find `missing-helpr` in this scope
 --> <input 3>:1:29
  |
1 | (defmacro bad () (template (missing-helpr 1)))
  |                             ^^^^^^^^^^^^^ not found in this scope
  |
 ::: <input 2>:1:12
  |
1 | (defn f () (bad))
  |            ----- in this macro call
  |
  = note: in this expansion of `bad`
```

The primary label lies in `<input 3>`, the new input, so the prompt rejects the new `bad` and keeps `f`. By the call label alone, `f` would be set aside and the broken macro accepted.

An error inside a module file, including one whose call and template are both in the module, is located at the earliest input that `use`s the module.

### 2. The loop

1. The new input replaces every input that binds a name it binds, and takes the place of the first one, as in RFC 0047 rule 6.
2. Check the buffer as `rslisp check` checks a file, and derive every input's names from its current expansion.
3. If an error is located at the new input, reject the input: print the errors located at the new input, and keep the buffer as it was before step 1.
4. If an error is located at an earlier input, set aside each earlier input that an error is located at, and go to step 2.
5. Otherwise the buffer checks. Print the warnings of rule 3 and the new input's own warnings as one report.

A check stops at the first stage that reports an error, so a later round can reveal errors that an earlier round hid, and can find a reference to a name that an earlier round set aside. A warning located at an earlier input does not print, as today.

**The loop ends.** Step 4 removes at least one earlier input, and no step adds one. After at most as many rounds as the buffer had inputs, no earlier input is left. A check of the new input alone has no error located at an earlier input, so it reaches step 3 or 5. A check costs about 20 ms (RFC 0047).

The loop covers a change on disk with no step of its own. When a module file changes, the next definition input's first round finds the module's errors, located at the inputs that `use` it, and sets them aside. An expression never changes the buffer: it runs against the buffer, or fails with every error the check reports, as today. After a module file changes, an expression's error located at an earlier input gets the help `enter a definition to set aside the inputs that no longer check`, whose wording belongs in `ExpandProblems`.

### 3. The warning

**Each input that the loop sets aside, and each replaced input that binds a name the new input does not, gets one `L0249` once the new input joins the buffer.** A replaced input whose names the new input all binds gets none. Its wording belongs in `ExpandProblems`, in place of `dropped_by_replacement`, which the prompt no longer needs. `L0249` is the first free code of the `L02xx` range, the range of `L0203`'s replacement.

| Part | Content |
| --- | --- |
| Header | ``warning[L0249]: set aside `karl` after `<input 4>` `` |
| Location | the primary label of the input's first error |
| Primary label | the first error's primary label and message |
| Secondary labels | the first error's secondary labels, in order |
| Notes | the first error's code and title, as `` error[L0409]: missing field `email` in `user` ``, then its own notes |
| Help | the first error's helps, then ``enter `karl` again to use it`` |

The title lists the names that the input bound before the round, in order, at most three, then `and N more`, as `` `a`, `b`, `c` and 2 more ``. An input that bound no name is named by its source, as `` `<input 2>` ``. The title always ends `` after `<input N>` ``, naming the new input. The error note, or for a dropped name the replaced note, says why, so the title never names a macro, a module or one of the new input's names.

The warnings print as one report, as every diagnostic does: in source order and capped.

```text
> (defstruct user :name str)
> (def karl (->user :name "karl"))
> (defn greet () (str "hi " (:name karl)))
> (defstruct user :name str :email str)
warning[L0249]: set aside `karl` after `<input 4>`
 --> <input 2>:1:11
  |
1 | (def karl (->user :name "karl"))
  |           ^^^^^^^^^^^^^^^^^^^^^ each missing field needs a value
  |
  = note: error[L0409]: missing field `email` in `user`
  = help: enter `karl` again to use it

warning[L0249]: set aside `greet` after `<input 4>`
 --> <input 3>:1:34
  |
1 | (defn greet () (str "hi " (:name karl)))
  |                                  ^^^^ not found in this scope
  |
  = note: error[L0201]: cannot find `karl` in this scope
  = note: `<input 4>` set aside `karl`: missing field `email` in `user`
  = help: enter `greet` again to use it
```

The original error is a part of the warning, not a second diagnostic: an `error[...]` header would say that something was rejected, which is the confusion this RFC removes.

A change on disk gives the same warning. After the file `shapes` stops exporting `square`, one round finds both errors:

```text
> (use shapes circle square)
> (defn area () (square))
> (defn z () 1)
warning[L0249]: set aside `circle` and `square` after `<input 3>`
 --> <input 1>:1:20
  |
1 | (use shapes circle square)
  |                    ^^^^^^ not exported
  |
  = note: error[L0205]: module `shapes` does not export `square`
  = help: add `square` to the `export` list of `shapes`
  = help: enter `circle` and `square` again to use them

warning[L0249]: set aside `area` after `<input 3>`
 --> <input 2>:1:16
  |
1 | (defn area () (square))
  |                ^^^^^^ not found in this scope
  |
  = note: error[L0201]: cannot find `square` in this scope
  = note: `<input 3>` set aside `square`: module `shapes` does not export `square`
  = help: enter `area` again to use it
```

#### Dropped names

**A replacing input takes effect even when it binds fewer names than the inputs it replaces.** Each replaced input that binds a name the new input does not gets the same warning, with the dropped names in its title, a primary label `set aside` on the first dropped name, and the note `` `->point` replaced the input that defined `point` `` in place of an error. The prompt no longer reports `L0203` for a dropped name. A name repeated inside one input is still `L0203`, as in a file. None prints if the loop rejects the new input.

```text
> (defstruct point :x i64 :y i64)
> (defn ->point (x y) (vec x y))
warning[L0249]: set aside `point` after `<input 2>`
 --> <input 1>:1:12
  |
1 | (defstruct point :x i64 :y i64)
  |            ^^^^^ set aside
  |
  = note: `->point` replaced the input that defined `point`
  = help: enter `point` again to use it
```

### 4. Later references

**The prompt records each name that leaves the buffer,** with the input that made it leave and a reason, and adds one note to each `L0201` that reports a recorded name, in a definition or in an expression. A name is recorded in its namespace, so a set-aside type does not annotate a value of the same name.

- A name of a set-aside input is recorded with the first error's title, or with the dropped-name note. Its note reads `` `<input 4>` set aside `greet`: cannot find `karl` in this scope ``.
- A name that an input stops binding while the input still checks goes without a warning. Its note reads `` `<input 3>` no longer defines `foo` after `<input 4>` ``.

```text
> (defn pick (a b) a)
> (defmacro mk (a b) (template (defn (unquote (pick a b)) () 1)))
> (mk foo bar)
> (defn pick (a b) b)
> (foo)
error[L0201]: cannot find `foo` in this scope
 --> <input 5>:1:2
  |
1 | (foo)
  |  ^^^ not found in this scope
  |
  = note: `<input 3>` no longer defines `foo` after `<input 4>`
```

The records of a definition are complete when the loop ends, so the notes join every diagnostic that prints after it, the warnings of rule 3 included. **After each definition that joins the buffer, the prompt drops the record of each name that the buffer binds,** whichever input binds it. A rejected input changes no record. If the loop rejects the new input after rounds that set inputs aside, the buffer keeps those inputs, and the new input's `L0201` for a name they bind gets the note `` `<input 5>` would set aside `karl`: missing field `email` in `user` ``.

**Two inputs can come to bind one name** after a redefinition, as `(defn bar () 2)` after `(mk foo bar)` once `pick` returns its second argument. The check reports `L0203` at the later input in buffer order, so the loop sets that input aside, even when it is the hand-written one. Its name is not recorded, because the buffer still binds it.

### Prior art

| System | A value or a definition that depends on a redefined type or function |
| --- | --- |
| Clojure | A call goes through a var, so a caller sees the new function. A `defrecord` makes a new JVM class, so a value built before keeps the old class. A protocol call on an old instance dispatches to the old extension, and a new instance has no extension until the `extend` is evaluated again. ([datatypes](https://clojure.org/reference/datatypes), [a report](https://groups.google.com/g/clojure/c/YHkCEUq1Xs0)) |
| GHCi | A new `data T` shadows the old one. Earlier bindings keep the old type, which GHCi prints as `Ghci1.T`, and mixing them gives a type error between `T` and `Ghci1.T`. ([GHCi](https://downloads.haskell.org/ghc/latest/docs/users_guide/ghci.html), [#8649](https://mail.haskell.org/pipermail/ghc-tickets/2014-January/008702.html)) |
| IEx | A new `defmodule` replaces the module with "warning: redefining module". Calls reach the new code. A struct is a map, so a value bound before keeps the old shape and fails only when used. ([Elixir Forum](https://elixirforum.com/t/rename-struct-field-how-to-get-iex-to-pick-it-up/1562)) |
| Julia | Redefining a method recompiles its callers (world age). From 1.12, a `struct` can be redefined, following world-age semantics. ([JuliaCon 2025](https://pretalx.com/juliacon-2025/talk/TJXV9L/), [1.12 release](https://www.i-programmer.info/news/98-languages/18431-julia-112-adds-trim-feature.html)) |
| Jupyter | A cell runs when the user runs it, and nothing tracks which cells depend on it. Running a `class` statement again binds the name to a new class object, and no cell that used the old one runs again. Reactive notebooks such as Pluto.jl delete a cell's globals when its definition is removed and run every dependent cell again. ([Python: class definitions](https://docs.python.org/3/reference/compound_stmts.html#class-definitions), [Pluto: reactivity](https://plutojl.org/en/docs/reactivity/)) |

Every one lets the redefinition take effect. The ones with state keep stale values that fail later, far from their cause. The prompt has no state but its text, so it can do what Pluto does: find each input that no longer checks at the moment of the redefinition, and name it there.

## Decision

The project owner chose option B on 2026-11-04, after the session in the Problem section at the prompt of pull request 299: the new definition takes effect, and each earlier input that no longer checks leaves the buffer with a warning that names it.

On 2026-11-04 the owner also decided that a redefinition wins when it would drop a name. A replacing input that defines fewer names than the inputs it replaces takes effect, each dropped name is set aside with the same `L0249`, with the note `` `->point` replaced the input that defined `point` ``, and the loop then sets aside each earlier input that no longer checks. The prompt no longer reports `L0203` for a dropped name.

## Rejected options and why

- **A: reject the input:** the most recent input loses to older ones, the error never names the rejected input, and the user must redefine every dependent input first, in an order they cannot see.
- **C: keep old definitions alive:** the buffer is text compiled whole for each expression, so two types of one name need versioned names in three stages. The prior art shows that stale values fail later, far from their cause.
- **D: set aside by reference:** it removes inputs that still check, and the check already knows which inputs fail.
- **Rejecting a replacement that drops a name with `L0203`:** it is option A for one more case, where the earlier input wins over the one the user just typed.
- **Locating every error by one label alone:** by the primary label, a call to a broken macro would set the macro aside and then reject the call with `cannot find`. By the call label, a broken macro would set its callers aside and be accepted.
- **A reason per cause in the title,** naming a macro, a changed module or the new input's names: each cause needs a case of its own, and the error note already says why.
- **Restoring a set-aside input when it checks again:** the buffer would change without an input, and the user could not tell which inputs it holds.

## Rule

The build that lands this RFC states the rule in [docs/language.md: The prompt](../docs/language.md#the-prompt), replacing RFC 0047 rule 6's rejections after a failed check and for a dropped name, and adds `L0249` to the diagnostics.
