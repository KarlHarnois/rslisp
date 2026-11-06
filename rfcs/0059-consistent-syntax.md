# RFC 0059: One consistent syntax

**Status:** Accepted
**Date:** 2026-11-06

## Problem

The language grew one form at a time, so its forms disagree on small rules that a reader must then learn one by one. A survey of `origin/main` at `950d77a5` (v0.8.6) read `prelude/`, `examples/` and `tests/fixtures/` with a reader that skips strings and comments, and left out 9 unreadable or too-deep fixture files of 1,881. It found these inconsistencies:

| Finding | Evidence |
| --- | --- |
| A struct field has two spellings: `:x f64` and `(x f64)`, `:x 1` and `(x 1)`. | 111 `struct`, 159 `new`, 29 payload clauses |
| Kernel forms sit beside the prelude forms that replace them. | 111 `struct`, 159 `new`, 203 `field`, 19 `-at` |
| Some operators work as values and some do not, so `+` needs the twin `add`, against RFC 0046's rule 5. | 10 twin names, at most 103 fixture uses |
| Four forms define a name without `def`. | 243 `enum`, 163 `trait`, 70 `rust-type`, 80 `rust-trait` |
| A `match` arm takes one body form, and every other clause takes many. | 1,130 arms, 26 of them with a `do` |
| Destructuring has three shapes, none of them RFC 0058's binding list. | 35 `let-pair`, 15 `let-fields` |
| Struct patterns bind by position, and every other field use has a name. Reordering two fields of one type swaps the bindings with no error. | 8 of 15 `let-fields` skip a field with `_` |
| A keyword argument is a label or a value, decided by the field's type. | 7 of 1,646 constructor calls |
| `max` and `min` take one iterator and not two numbers. | 14 uses |
| Printing has holes: no `eprint`, and `prn` takes one argument. | 3,032 `println`, 315 `prn` |
| `update` takes a map only, while `assoc` takes a struct too. | 7 `update`, 95 `assoc` |
| The `code` module's `take`, `drop` and `map-list` put the count or the function first, and program code puts the data first. | 8 `take`, 26 `drop`, 32 `map-list` |
| Internal helpers can be imported, and generic lookup names sit in every module. | 21 helpers, at most 23 uses of `header` and `page-body` |

The language cannot fix these inside one form, because each fix changes what an earlier RFC or decision recorded, and most fixes move forms across the whole repository.

## Options

The survey weighed these options. Each row names the choice the decision below takes.

| Question | Options | Taken |
| --- | --- | --- |
| Field spellings | keep both; flat fields in every form a user reads; also drop the bare label from kernel `new` | flat fields |
| Kernel forms beside prelude forms | keep; hide `struct` and `new`; hide the three `-at` forms; hide them all | hide them all, `enum` and `trait` too |
| `defenum` and `deftrait` | rename the kernel forms; prelude macros over the kernel forms | prelude macros |
| Rust-backed definitions | `defrust-type` and `defrust-trait`; `deftype` and `deftrait` with a `(rust ...)` clause | the clause |
| Operators as values | keep the twins; operators only; operators and `some`, `ok`, `err` | operators and constructors |
| `match` arms | one body; several body forms | several |
| Destructuring | keep both forms; patterns in `let`; one `let-match` | `let-match` |
| Struct patterns | positional only; add keyword patterns | add them |
| Keyword arguments | keep rule 2 of the constructor; drop it | drop it |
| `max`, `min` | keep; new names `max-item` and `min-item`; dispatch by argument count | dispatch |
| Variants with named fields | positional only; keyword labels too | keyword labels too |
| `max`, `min` over values | `ord` only; floats too | floats too |
| `neg` | a twin that leaves `export`; stays exported | stays exported |
| Template of `max`, `min` over values | one runtime function; clauses by type; clauses by trait | clauses by trait |
| Imports | keep; move helpers out; rename `header` and `page-body` | both |

## Recommendation

The survey recommended each option taken above, with two differences. It recommended `defrust-type` and `defrust-trait`, because they add no grammar. It recommended moving the helpers only when convenient. The project owner decided both differently, and the `deftrait` prototype below shows that the `rust` clause holds. The questions of decisions 15 to 18 came up in the first review of this RFC, so the survey made no recommendation on them.

## Decision

The project owner made these 18 decisions on 2026-11-06, decisions 15 to 18 after the first review of this RFC.

1. **Definers.** A form that defines a name starts with `def`. A form that only extends something does not, so `impl`, `derive`, `type`, `use`, `export`, `document` and `header-shape` keep their names.
2. **`defenum` and `deftrait`.** Prelude macros over the kernel's `enum` and `trait`, as `defstruct` is over `struct`. Each takes a docstring for the type it defines, checks its own shape so an error names `defenum` or `deftrait`, and `rslisp types` prints the macro's name.
3. **Rust-backed definitions.** A definer with a `(rust "Path")` clause names an existing Rust thing instead of defining a new one, as `enum` does today. `rust-type` becomes `(deftype (boxed item) (rust "Box<{0}>"))`, and `rust-trait` becomes `(deftrait number (rust "crate::rt::Number") i8 i16)`. `deftype` without a `rust` clause stays free for a future type alias. Prototype `deftrait` with both body shapes first. If its errors or docs get unclear, the RFC falls back to `defrust-trait` for traits only, and says why.
4. **Hidden kernel forms.** The prelude stops re-exporting `struct`, `new`, `field`, `assoc-at`, `set-at`, `map-at`, `enum` and `trait`. A file that needs one writes `(use kernel field)`. `rslisp doc new` falls back to the `kernel` module and prints the page, marked as needing `(use kernel new)`.
5. **Fields and bindings.** A field is always a keyword, and a binding is always a bare name. An enum variant with named fields uses the flat spelling of `defstruct`: `(defenum shape (labeled :text string :size f64))`. Users read a field with `(:name p)`.
6. **`type` stays.** It annotates a name and defines none.
7. **Operators as values.** `+ - * / < <= > >= = not=` and the constructors `some`, `ok` and `err` also work as values, as `str` does, so `(reduce (iter v) 0 +)` and `(map (iter v) some)` run. The twins `add sub mul div lt le gt ge eq ne` stop being exported. `and`, `or` and `println` stay macros. This reverses RFC 0046's decision to keep the twins.
8. **`match` arms.** An arm takes several body forms, as `cond` does, lowered as a `do`. `L0232` retires.
9. **`let-match`.** One `let-match` with RFC 0058's binding list and `match`'s patterns replaces `let-pair` and `let-fields`: `(let-match ((pair word count) (pair "apples" 3) (point x _) p) body...)`. A refutable pattern reports the missing case, as a one-arm `match` does. It lands with or after RFC 0058's build and takes the `:bindings` header-shape marker.
10. **Keyword struct patterns.** `(point :x a)` binds by name and skips the fields it leaves out, emitting `Point { x: a, .. }`. Positional patterns stay.
11. **A keyword argument is always a label.** `(->sorter :name "by name")`, which today passes the `:name` reader by position, becomes the error that `sorter` has no field `name`. Positional calls such as `(->point 1 2)` stay.
12. **`max` and `min`.** One argument is an iterator, and the result is an `option`, as today. Two or more arguments are values, so `(max 1 2)` is `2`.
13. **What users can import.** The internal helpers of `prelude/syntax` and `prelude/macros` move to a prelude-internal module that users cannot import. `header` and `page-body` become `doc-header` and `doc-body`. The `case` and `dotimes` examples in `docs/language.md` get fixed or marked as code the reader writes.
14. **Smaller fixes.** Add `eprint`, and let `prn` take several arguments. `update` takes a keyword key to update a struct field. The `code` module's `take`, `drop` and `map-list` put the data argument first. `else` in `cond` and `_` in `match` both stay.
15. **Variant labels.** Enum variants with named fields also take keyword labels, as structs do: `(shape/labeled :text (str "hi") :size 2.0)` builds, and `(match s ((shape/labeled :text t) ...))` binds by name and skips the fields it leaves out. Positional building and matching stay.
16. **`max` and `min` over floats.** `max` and `min` over two or more values accept floats: `(max 1.0 2.0)` is `2.0`, as Rust's `f64::max` allows.
17. **`neg`.** `neg` stays exported, as the value form of unary minus, because it is a different operation from `-`, not a twin: `(map (iter v) neg)` runs.
18. **Clauses by trait.** A `rust` template clause may name a trait instead of a type, so an `ord` clause covers every concrete `ord` type, program types and references included, and the default covers what is left. `max` and `min` over values use it: `ord` types emit `std::cmp::max(a, b)`, `f32` and `f64` emit `f32::max(a, b)` or `f64::max(a, b)`, and only a generic parameter bounded by `partial-ord` and not `ord` falls back to `crate::rt::max`.

Before 1.0 the owner is the only user, so the design adds no migration aid, and the build migrates the repository's own code.

### Rules and costs

Each "today" example ran on a release build of `origin/main` at `ee0df4b7`. The prototype sweep, nine of those examples' error codes and the examples of decisions 15 to 18 ran at `e31f99e3`, which changes only doc pages and the version. Each "after" example shows the shape after the build, unless its text says that a prototype ran it. Counts come from the survey. Decision 1 is the rule that decisions 2 and 3 apply. Decision 6 and the `else` and `_` part of decision 14 change no form.

#### Decisions 2 and 5: `defenum`, `deftrait` and flat fields

**Rule:** a definer's name starts with `def`. A type's docstring is the first form after its head. A variant is a name, a list of types, or a name and `:field type` pairs. A variant whose second item is a keyword holds `:field type` pairs, and any other variant's items are types.

```lisp
; today
(enum shape
  (circle f64)
  (labeled (struct (text string) (size f64)))
  dot)

(trait area-of
  (area (fn ((ref self)) f64)))

; after, which the prototype ran
(defenum shape
  "A shape."
  (circle f64)
  (labeled :text string :size f64)
  dot)

(deftrait area-of
  "Shapes with an area."
  (area (fn ((ref self)) f64)))
```

**Costs:**

- 243 `enum`, 163 `trait` and 29 payload clauses change. Snapshots that mention each name are at most 207 and 146 files, many of them in a Rust line or in prose.
- The compiler gains no form. `rslisp types` prints the name of the macro that defined the enum, read from the macro origin as `lower_enum` reads it today, so the Rust names no prelude macro. `show_variant` prints flat fields for it. `rslisp expand` keeps printing the kernel form.
- The macros state `(header-shape (defenum name) :docstring :one-per-line)` and `(header-shape (deftrait name) :docstring)`. A form with `:docstring` stays on one line only when its body is one form, so `(defenum color red green)` breaks where `(enum color red green)` does not, as `defstruct` already does.
- Each macro declares its names before its checks, as `defstruct` does: the type, each method of a trait, and each variant under its qualified and its short name. A failed definer then reports one error. The prototype's failed `deftrait` adds no `L0201` at a call of a method it declared.
- A declared name alone does not cover a pattern, because `lower_list_pattern` reports `L0231` for a head that is not a variant, so a failed `defenum` would add `L0231` at each `(circle r)`. A failed `enum` reports no `L0231` today. The compiler gains one condition: `variant_pattern_head` also accepts a binding that a failed definer declared. The pattern's parts lower as usual, so `r` binds and the arm's body reports nothing, and no stage after the expander reads the pattern, because the program has an error. On a stand-in definer that declares those names and fails, `(match s ((circle r) ...) ((shape/circle r) ...))` reports `L0301` and `L0231` today, and only `L0301` with a scratch patch of that condition.
- Diagnostics added: one macro-stage code for each new shape error, at the next free numbers. `L0301` is only the prototype's placeholder.
- The emitted Rust does not change.

#### Decision 3: Rust-backed definitions

**Rule:** `(rust "Path")` comes right after the head or the docstring. With the clause, a definer names a Rust thing, and the forms after the clause are what the Rust thing needs. Without it, `defenum` and `deftrait` define a new thing, and `deftype` is an error.

```lisp
; today
(rust-type (boxed item) "Box<{0}>")
(rust-trait number "crate::rt::Number" i8 i16)
(rust-type (non-zero n) "std::num::NonZero<{0}>" (u8 "std::num::NonZeroU8"))

; after, which the prototype ran with a user path in place of the runtime's
(deftype (boxed item) (rust "Box<{0}>"))
(deftrait number (rust "crate::rt::Number") i8 i16)
(deftype (non-zero n) (rust "std::num::NonZero<{0}>") (u8 "std::num::NonZeroU8"))
```

`method`, `structural`, `help`, `associated`, `where` and `with` keep their meaning after the clause. The clause is told from a method as `enum` tells it, because no type is a string, so a method named `rust` still works.

**Costs:**

- 70 `rust-type` and 80 `rust-trait` forms change, nearly all in interop fixtures and the prelude. Snapshots that mention each name are at most 44 and 67 files.
- 14 helps in `expand/src/expand/problems.rs` and `src/types/problems.rs` name `rust-type` or `rust-trait`, and each must name the form the user wrote, through the macro origin.
- `deftrait` holds two body grammars, and `deftype` reads as a neighbor of `type`. The owner accepted both.
- The emitted Rust does not change.

#### Decision 4: hidden kernel forms

**Rule:** a kernel form that a prelude form replaces is not bound in a program until the file imports it.

```lisp
; today
(struct point (x i64))
(println (field (new point (x 1)) x))

; after, and this runs today too
(use kernel field new struct)
(struct point (x i64))
(println (field (new point (x 1)) x))
```

**Costs:**

- `rslisp check` differs on 170 of 1,676 fixture files when the six struct and map names leave the `export` list. All 170 write a hidden name or import a module that does. Nothing breaks through a prelude macro, because a prelude macro resolves the kernel name where it was defined.
- An enum `(struct ...)` payload needs `struct` bound, so the 29 payload clauses move first.
- `prelude/prelude.rslisp` loses the names from its `export` list. `rust-type` and `rust-trait` leave it too, as open detail 1 says.
- `rslisp doc` looks a name up in `kernel` when the program does not bind it, and prints the page with a line that names the `use`. `tests/cli.rs` covers it.
- `docs/language.md` writes `(field p x)` in a `display` example, and it becomes `(:x p)`. The sentence that the prelude re-exports the kernel changes.
- The report of `rslisp test prelude/tests --untested prelude` changes with the `export` list.
- The emitted Rust does not change.

#### Decisions 7 and 17: operators as values

**Rule:** an operator or a constructor in value position is a function, as `str` is. Its value form takes two arguments, or one for a constructor. `neg` is the value form of unary minus, and `-` as a value is subtraction.

```lisp
; today: L0304, with the help to pass `add`
(reduce (iter v) 0 +)

; today: L0304
(map (iter v) some)

; today, and after
(map (iter v) neg)

; after
(reduce (iter v) 0 +)
(map (iter v) some)
```

```rust
v.iter().fold(0, |argument: i64, argument_2: &i64| argument + argument_2)
v.iter().map(|value: &i64| Some(value))
v.iter().map(|argument: &i64| -argument)
```

`(reduce (iter v) 0 add)` emits the first line today, and `(fn (value) (some value))` emits the second, so a value form emits what a program spells out by hand today.

**Costs:**

- `+ - * / < <= > >= = not=` move from a plain `defrule` to a form that answers in value position. `some`, `ok` and `err` do the same. Open detail 2 names the choice.
- At most 103 fixture uses of `add`, `sub`, `mul`, `div` and `eq` change, many of them a fixture's own definition, which then needs no change. `lt`, `le`, `gt`, `ge`, `ne` and `div` have no head use in fixtures.
- `eq` is also a trait name, as open detail 11 says.
- `neg` changes nothing: it stays in `export`, and `(- x)` keeps calling it.
- `rem` stays, because it has no operator. `(sort! v >)` keeps reading a written `>` by binding, so it still emits `.sort_by(|left, right| right.cmp(left))`.
- The help of `L0304` loses the case that names `add`. `L0304` stays for `and`, `or`, `println` and every other macro.
- `(+ 1 2)` in macro code reports ``L0306: `add` belongs to the program`` today, which would name a form that is no longer exported. `L0306` names the operator the user wrote, and the keys of `CODE_REPLACEMENTS` in `expand/src/expand/problems.rs` move from `add sub lt le gt ge eq` to the operators.
- The emitted Rust of existing calls does not change.

#### Decision 8: `match` arms

**Rule:** an arm is `(pattern body ...)`. The body is a `do`: its last form is the arm's value and is in tail position.

```lisp
; today: L0232
(match 1 (1 (println "a") 2) (_ 3))

; after
(match 1 (1 (println "a") 2) (_ 3))
```

```rust
match 1_i64 {
    1 => {
        println!("a");
        2_i64
    }
    _ => 3,
}
```

An arm with no body, as in `(match x (1) (_ (do)))`, is `unit`, as a `cond` clause with no body is today. A `defrule` that writes `(do body ...)` in an arm emits the Rust above.

**Costs:**

- `lower_arm` lowers a slice of forms in place of one form.
- `L0232` leaves `expand/src/diagnostic/code.rs`, with the `match_arm_body` fixture and snapshot.
- `display-match` reads `(nth arm 1)`, so it would drop every later form of an arm. Open detail 3 covers it.
- The 26 `do` forms become optional.
- An arm that holds a mistake such as `(1 2 3)` now reads as a body and no longer reports an error.

#### Decision 9: `let-match`

**Rule:** `let-match` takes a binding list whose names are `match` patterns, and a body. Bindings are sequential, as in `let`.

```lisp
; today
(let-pair (word count) (pair "apples" 3)
  (let-fields (point (x _)) p
    (println word x)))

; after, which a `defrule` prototype ran
(let-match ((pair word count) (pair "apples" 3)
            (point x _) p
            total (+ x count))
  (println word)
  (println total))
```

```rust
let (word, count) = ("apples", 3_i64);
let Point { x, .. } = p;
let total = x + count;
println!("{}", word);
println!("{}", total)
```

`(let-match ((some x) (some 1)) ...)` reports `L0433` with the missing case `none`, as a one-arm `match` does.

**Costs:**

- 50 uses change, all of them in `tests/fixtures/`, and `let-pair` and `let-fields` leave the prelude with their header shapes.
- The form states `(header-shape (let-match bindings) :bindings)`. It reports `L0334`, `L0335` and `L0336` for its own form, as `let-try` and `loop` do.
- It expands every pair in one step, as RFC 0058's `let` does, because the recursive prototype would stop at `L0326` on a long list.
- The emitted Rust is the Rust of a one-arm `match`, so it does not change.

#### Decision 10: keyword struct patterns

**Rule:** a struct pattern is positional, with one pattern per field, or it is `:field pattern` pairs that name any of the fields. The two do not mix, as open detail 12 says.

```lisp
; today: L0244
(match p ((point :x a) (println a)))

; today
(match p ((point a _) (println a)))

; after
(match p ((point :x a) (println a)))
```

```rust
let Point { x: a, .. } = p;
```

**Costs:**

- `lower_list_pattern` keeps the keywords of a struct pattern, and `struct_pattern` in `src/types/infer.rs` places each pattern by field and fills the rest with wildcards, so coverage and emission take the shape they already take.
- `L0244` stays for every binder that is not a struct or variant pattern. The type stage adds codes for an unknown field, a repeated field and a pattern that mixes the two forms.
- The emitted Rust does not change, because `(point a _)` emits this `let` today.

#### Decision 11: a keyword argument is always a label

**Rule:** in a call of a `->name` constructor or a function with keyword parameters, a keyword argument is a label. A call by name passes a field reader as a value, after its label.

```lisp
(defstruct sorter :key (fn ((ref user)) (ref str)) :label str)

; today: positional, and `:name` is the value of `key`
(->sorter :name "by name")

; after: `sorter` has no field `name`
(->sorter :name "by name")

; today and after
(->sorter :key :name :label "by name")
```

The rule covers only those calls and, by decision 15, a variant with named fields. A keyword stays a value elsewhere, as in `(map (iter points) :x)`.

**Costs:**

- Rule 2 leaves the call reader in `prelude/syntax.rslisp`, with `function-type?`, and each list in `docs/language.md` loses a step.
- 7 fixtures change. The unknown-field error of the rule's example exists today.
- A keyword outside a pair, as in `(->wrap :name)`, runs today. Open detail 4 covers it.
- The emitted Rust of every call that stays valid does not change.

#### Decisions 12, 16 and 18: `max` and `min`

**Rule:** the argument count picks the meaning, as it does for `reduce`. One argument is an iterator whose items implement `ord`, as today, so `(max (iter floats))` stays `L0404`. Two or more arguments are values of one type that implements `partial-ord`, which covers every `ord` type, `f32` and `f64`, and a longer call nests. Decision 18's clauses pick the Rust. `min` is the same with its own names.

```lisp
; today, and after
(max (iter v))

; today: L0402
(max 1 2)
(max 1.0 2.0)

; after: a call with two or more arguments expands to this definition
(def larger
  (rust "crate::rt::max({0}, {1})"
    (where ((a partial-ord)) (fn (a a) a))
    (f32 "f32::max({0}, {1})")
    (f64 "f64::max({0}, {1})")
    (ord "std::cmp::max({0}, {1})")))

(max 1 2)
(max 1 2 3)
(max 1.0 2.0)
```

A scratch patch of decision 18 ran this definition through rustc, with a user path in place of the runtime's, and each row is the Rust it emitted:

| The binder's type at the call | Emitted Rust |
| --- | --- |
| `i64` | `std::cmp::max(1_i64, 2_i64)` |
| `i64`, three arguments | `std::cmp::max(std::cmp::max(1_i64, 2_i64), t)` |
| `(ref i64)` | `std::cmp::max(&t, &t)` |
| `(ref str)` | `std::cmp::max("a", "b")` |
| `string` | `std::cmp::max("a".to_string(), "b".to_string())` |
| `(pair i64 i64)` | `std::cmp::max((1_i64, 2_i64), (1_i64, 3_i64))` |
| a struct that derives `ord` | `std::cmp::max(make_ticket(1), make_ticket(2))` |
| `f64` | `f64::max(1.0_f64, 2.0_f64)` |
| a generic `(a ord partial-ord)` | `std::cmp::max(x, y)` |
| a generic `(a partial-ord)` | `crate::rt::max(x, y)` |
| a generic `(a float)` | `crate::rt::max(x, y)` |

The runtime holds the fallback:

```rust
pub fn max<T: PartialOrd>(left: T, right: T) -> T {
    if left > right || right != right { left } else { right }
}

pub fn min<T: PartialOrd>(left: T, right: T) -> T {
    if right < left || left != left { right } else { left }
}
```

On equal values these return what `Ord::max` and `Ord::min` return, and when one float is NaN they return the other, as `f64::max` and `f64::min` do, so a generic call gives the result that a concrete call gives.

**Costs:**

- `max` and `min` move from `def` in `prelude/iterator.rslisp` to macros that answer in value position. The 14 uses keep working.
- `L0402` no longer fires for two arguments.
- The runtime gains the two functions, which `src/emit/runtime.rs` adds only to a program whose emitted Rust calls one.
- A function bound by `(a ord)` alone reports `L0440` at `(max x y)`, with the help to add `(a partial-ord)`, as `(< x y)` does there today.
- `partial-ord` also admits a pair of floats, which compares field by field and takes the default template.

#### Decision 13: what users can import

**Rule:** a `use` outside the prelude cannot name the prelude-internal module. `doc-header` and `doc-body` name the two parts of the page that `doc-page` returns.

```lisp
; today
(header twice)
(page-body twice)
(use prelude/macros whole-pairs?)

; after
(doc-header twice)
(doc-body twice)
; the `use` is an error
```

**Costs:**

- The 21 helpers move, as open detail 5 says, and `prelude/macros`, `prelude/syntax`, `prelude/struct`, `prelude/assoc`, `prelude/test` and `prelude/prelude` import them from there. No fixture imports one of the 8 `prelude/macros` helpers. `(use prelude/syntax definition)` appears once, in `runtime_call_to_a_macro_helper`, and that fixture moves to a public helper.
- `use` gains one check in the module loader, and one `L02xx` code. Open detail 5 names its shape.
- `HEADER_MACRO` and `BODY_MACRO` in `src/cli/doc.rs` and the two lookups in `expand/src/expand/prelude_state.rs` change with the prelude, at most 23 uses in all.
- The two `docs/language.md` examples need a sentence, or code that runs.
- `map-list`, `document`, `header-shape`, `deftest` and `assert` stay public and keep needing a `use`.
- The emitted Rust does not change.

#### Decision 14: smaller fixes

**Rule:** `eprint` is `print` on standard error. `prn` prints each argument's printed form, and takes at least one. A keyword key in `update` names a field, and takes no default. Data comes first in the `code` module, as it does in a program.

```lisp
; today: L0201, and L0402, and L0401
(eprint "x")
(prn 1 2)
(update p :x f 0)

; today: the count and the function first
(take 2 items)
(drop 2 items)
(map-list f items)

; after
(eprint "x")
(prn 1 2)
(update p :x f)
(take items 2)
(drop items 2)
(map-list items f)
```

A `defrule` prototype of `update` over `assoc` emits Rust's struct update:

```rust
let b = Item {
    count: {
        let c: i64 = a.count;
        c + 1
    },
    ..a
};
let c = Item {
    name: shout(&b.name),
    ..b
};
```

**Costs:**

- `eprint` is one prelude macro. `prn` becomes a macro that keeps its value form, because `prn` works as a value today.
- `rslisp eval` and the prompt call `prn` as a function and read its `where` bound, as rules 3 and 4 of RFC 0047 state. `PRINTER` in `src/cli/evaluation_output.rs` names `prn`, `result_printer.rs` builds a one-argument call of it, and `src/types/form_results.rs` reads the bound from its typed definition. A macro has no typed definition, so every result, a closure included, would count as printable. The one-argument function moves to `print-form`, of the same type as `prn` today, and `PRINTER` follows it. `prn` expands to it. `prelude/text` exports it and `prelude/prelude` does not, so a program gains no second name for `prn`, and the CLI's lookup reads that module.
- `(prn)` stays an error. It moves from `L0402` to a macro-stage code.
- `update` tests its key with `keyword?` and takes no default for a field, because a field always exists. `(update p :x f 0)` moves from `L0401` to a macro-stage code, whose help says to drop the default. A base that is not a name binds with `let` once, so `(update (build) :x f)` calls `build` once.
- A function that owns its argument cannot take a non-Copy field this way, as open detail 6 says.
- 8 `take`, 26 `drop` and 32 `map-list` calls change, nearly all in the prelude. The `take` and `drop` built-ins of the interpreter swap their parameters, and the `code` table in `docs/language.md` follows.

#### Decision 15: variant labels

**Rule:** a variant with named fields takes one positional item per field, or `:field item` pairs. A construction by pairs names every field, in any order, as a `->name` call without defaults does. A pattern by pairs names any of the fields and skips the rest. The short head of a pattern takes the same pairs, as `(labeled :text t)`.

```lisp
(defenum shape
  (circle f64)
  (labeled :text string :size f64)
  dot)

; today, in `enum`'s spelling, and after
(shape/labeled (str "hi") 2.0)
(match s ((labeled t _) (println t)) (_ (println "other")))

; today: L0402, and L0244
(shape/labeled :text (str "hi") :size 2.0)
(match s ((labeled :text t) (println t)) (_ (println "other")))

; after
(shape/labeled :text (str "hi") :size 2.0)
(match s ((labeled :text t) (println t)) (_ (println "other")))
```

Both forms emit the Rust that the positional form emits today:

```rust
Shape::Labeled {
    text: "hi".to_string(),
    size: 2.0
}
```

```rust
match s {
    Shape::Labeled { text: t, .. } => println!("{}", t),
    _ => println!("other"),
}
```

**Costs:**

- `lower_list_pattern` keeps the keywords of a variant pattern, and `named_variant_pattern` and `short_variant_pattern` in `src/types/infer.rs` place each pattern by field and fill the rest with wildcards, through the code that `struct_pattern` gains for decision 10.
- The expander keeps the labels of a variant call, and `infer_variant` places each argument by field. The emitter writes the fields in the written order, because Rust runs a struct literal's fields in that order.
- `L0402` stays for a positional call with the wrong count. A keyword on a positional variant, as in `(circle :radius r)`, reports decision 10's unknown-field code, whose help says that `circle` takes its payload by position, so no `L0244` help tells a user to rename a field they declared.
- A variant call with the fields out of order needs a fixture of its own, because emission keeps the written order.
- `show` keeps printing a variant positionally, which still builds an equal value.

#### Decision 18: clauses by trait

**Rule:** a clause of a `rust` form is `(target "template")`, and its target is a type or the name of a trait: `(ord "std::cmp::max({0}, {1})")`. A call takes the first clause in written order that matches the type its one binder takes. A type clause matches by equality, as today. A trait clause matches when that type implements the trait. If no clause matches, the call takes the default template. So a program writes the narrower clause first: `(i64 ...)` before `(ord ...)`.

The target is the trait's bare name, which has one form and needs no binder. `((where ((b ord)) b) "...")` stays `L0413`, because a `where` wraps a whole signature. A trait with a second type takes its plain bound: `(partial-eq "...")` is `A: PartialEq<A>`. `rust-type` clauses stay types only, because a `rust-type` picks a Rust type and no program needs a trait there.

**How the test works.** `RustClause.target` stays a `TypeExpr`, because the expander's `lower_type` already gives `TypeExprKind::Trait` for a trait's name. `collect_template_clauses` in `src/types/checker.rs` turns that kind into a trait target and sends every other kind through `lower_type`, which reports `L0449` for a trait today. `src/types/template_clauses.rs` then holds:

```rust
pub enum ClauseTarget {
    Type(Type),
    Trait(Bound),
}

pub fn chosen(
    &self,
    binding: BindingId,
    parameter: &Type,
    implements: &dyn Fn(&Bound) -> bool,
) -> Option<&Rc<str>> {
    self.clauses
        .get(&binding)?
        .iter()
        .find(|(target, _)| match target {
            ClauseTarget::Type(exact) => exact == parameter,
            ClauseTarget::Trait(bound) => implements(bound),
        })
        .map(|(_, template)| template)
}
```

`definition_template` in `src/emit/expressions.rs` passes a closure over `TraitTable::satisfies`, the test the checker uses for bounds. A concrete type goes through its `rust-trait` rows and their requirements, so `(ref i64)` matches `ord` through `(where ((a ord)) (ref a))`, and a struct matches through its written `derive`. A generic parameter matches when `program.generics` lists the bound on it, so `(a ord partial-ord)` takes the clause and `(a partial-ord)` does not. A type that the table leaves undecided takes the next clause. The test runs at emission, after every check, and every check before emission still reads the default template alone.

| Mistake | Report |
| --- | --- |
| A template that passes an argument unlike the default, as `(ord "{0}.max({1})")` under a call-form default | `L0247`, as today, which the scratch patch reported on a trait clause |
| A form with no binder or several, under a trait clause | `L0246`, as today, which the scratch patch reported on a trait clause |
| An unknown name, as `(nosuch "...")` | `L0201`, as today |
| A trait with a written second type, as `((partial-eq i64) "...")` | `L0212`, as today |
| A trait clause in a `rust-type` | `L0449`, as today |
| A template with clauses that no `def` binds | `L0603`, as today |
| A clause that an earlier clause covers, as `(ord ...)` then `(i64 ...)`, or the same target twice | a new type-stage code at the next free number, with the help to move the narrower clause first |

`L0247` keeps its rule, so a clause passes its arguments as the template does, and this is why floats emit `f64::max(a, b)` and not `a.max(b)`.

**Costs:**

- The kernel's `rust` row in `docs/language.md` and its clause paragraph change, and `TemplateClauses`, `collect_template_clauses` and `definition_template` change with them. The scratch patch of these three is 35 lines, without the new code.
- The new check needs each trait's implementors, so it runs in the type stage. The scratch patch does not hold it, and today a repeated target reports nothing.
- `tests/fixtures/` gains a run fixture for each row of the table above, and an error fixture for each new or newly reached report. `rust_type_clauses` emitted the same Rust under the scratch patch.
- A reader of a `rust` form now reads clause order. That order matters only where two targets overlap.
- A clause matches at emission, so no error says that a type took the default. The default must therefore compile for every type the bound admits, as it does today.

`clamp` is a second user. The prelude has no `clamp` today, and `(clamp x low high)` over values needs the same choice, because `Ord::clamp` needs `Ord` and floats have their own. The scratch patch ran `std::cmp::Ord::clamp(5_i64, 1_i64, 3_i64)`, `f64::clamp(5.0_f64, 1.0_f64, 3.0_f64)` and `std::cmp::Ord::clamp` on a struct that derives `ord`, from one definition with the three clause targets. Adding it is not part of this RFC.

### The `deftrait` prototype

**Result: `deftrait` holds with both body shapes, so the RFC does not fall back to `defrust-trait`.** The prototype is three `defmacro` forms in a user module that imports only `prelude/macros` and `code`, on `main` at `e31f99e3`. No prelude module that defines a type sits below `prelude/macros`, so a prelude module can hold the same macros and every other module can use them.

```lisp
(use prelude/macros defmacro documented-body? header-shape note-docstring template type-doc-key)
(use code bool-not concat declare-definition declare-type-definition drop empty? fail first keyword? len list list? nth prepend rest string string? symbol-name symbol? value-eq)
(export deftrait deftype defenum)

(def head-name (fn (head) (if (list? head) (first head) head)))

(def word? (fn (form word)
  (if (symbol? form) (value-eq (symbol-name form) word) false)))

(def rust-clause? (fn (form)
  (if (list? form)
    (if (empty? form) false
      (if (word? (first form) "rust")
        (if (value-eq (len form) 1) false (string? (nth form 1)))
        false))
    false)))

(def split-doc (fn (head body)
  (if (documented-body? body)
    (do (note-docstring (type-doc-key (head-name head)) (first body)) (rest body))
    body)))

(def check-no-late-rust (fn (forms)
  (if (empty? forms) (quote ok)
    (if (rust-clause? (first forms))
      (fail "the `rust` clause comes right after the name" (first forms) "move it to the first place after the name, or after the docstring")
      (check-no-late-rust (rest forms))))))

(def trait-method-names (fn (forms)
  (if (empty? forms) (list)
    (if (word? (first (first forms)) "type")
      (trait-method-names (rest forms))
      (prepend (first (first forms)) (trait-method-names (rest forms)))))))

(def check-methods (fn (forms)
  (if (empty? forms) (quote ok)
    (if (if (list? (first forms)) (if (empty? (first forms)) false (symbol? (first (first forms)))) false)
      (check-methods (rest forms))
      (fail "a `deftrait` method is a name and a type, as `(area (fn ((ref self)) f64))`" (first forms)
        "to name a Rust trait, write `(rust \"Path\")` after the name")))))

(def declare-all (fn (names)
  (if (empty? names) (quote ok)
    (do (declare-definition (first names)) (declare-all (rest names))))))

(def method-shaped? (fn (form)
  (if (list? form)
    (if (value-eq (len form) 2)
      (if (list? (nth form 1))
        (if (empty? (nth form 1)) false (word? (first (nth form 1)) "fn"))
        false)
      false)
    false)))

(def check-no-trait-methods (fn (forms)
  (if (empty? forms) (quote ok)
    (if (method-shaped? (first forms))
      (fail "a `deftrait` with a `rust` clause lists its implementors here" (first forms)
        "declare a method of a Rust trait as `(method name type)`")
      (check-no-trait-methods (rest forms))))))

(def rust-trait-form (fn (head clause more)
  (check-no-trait-methods more)
  (if (list? head)
    (fail "a Rust trait takes no supertraits" head "write the name alone")
    (if (value-eq (len clause) 2)
      (prepend (quote rust-trait) (prepend head (prepend (nth clause 1) more)))
      (fail "a `rust` clause on `deftrait` holds one string" clause "write it as `(rust \"Path\")`")))))

(def named-forms (fn (forms)
  (if (empty? forms) (list)
    (if (if (list? (first forms)) (if (empty? (first forms)) false (symbol? (first (first forms)))) false)
      (prepend (first forms) (named-forms (rest forms)))
      (named-forms (rest forms))))))

(def program-trait-form (fn (head body)
  (declare-all (trait-method-names (named-forms body)))
  (check-methods body)
  (prepend (quote trait) (prepend head body))))

(def trait-form (fn (head forms)
  (if (empty? forms)
    (program-trait-form head forms)
    (if (rust-clause? (first forms))
      (do (check-no-late-rust (rest forms))
          (rust-trait-form head (first forms) (rest forms)))
      (do (check-no-late-rust forms)
          (program-trait-form head forms))))))

(defmacro deftrait (head body ...)
  "Defines a trait, or names a Rust trait with a `rust` clause."
  (declare-type-definition (head-name head))
  (trait-form head (split-doc head body)))

(header-shape (deftrait name) :docstring)

(def type-form (fn (head forms)
  (if (empty? forms)
    (fail "`deftype` names a Rust type" head "write `(deftype name (rust \"Path\"))`")
    (if (rust-clause? (first forms))
      (prepend (quote rust-type) (prepend head (prepend (nth (first forms) 1) (rest forms))))
      (fail "`deftype` names a Rust type" (first forms) "write `(deftype name (rust \"Path\"))`")))))

(defmacro deftype (head body ...)
  "Names a Rust type."
  (declare-type-definition (head-name head))
  (type-form head (split-doc head body)))

(header-shape (deftype name) :docstring)

(def flat-fields (fn (items)
  (if (empty? items) (list)
    (prepend (list (first items) (nth items 1)) (flat-fields (drop 2 items))))))

(def variant-form (fn (variant)
  (if (list? variant)
    (if (word? (first variant) "rust") variant
      (if (value-eq (len variant) 1) variant
      (if (keyword? (nth variant 1))
        (list (first variant) (prepend (quote struct) (flat-fields (rest variant))))
        variant)))
    variant)))

(def variant-forms (fn (variants)
  (if (empty? variants) (list)
    (prepend (variant-form (first variants)) (variant-forms (rest variants))))))

(defmacro defenum (head body ...)
  "Defines an enum."
  (declare-type-definition (head-name head))
  (prepend (quote enum) (prepend head (variant-forms (split-doc head body)))))

(header-shape (defenum name) :docstring :one-per-line)
```

Both shapes ran: a trait with methods, a default method and supertraits in its head, and Rust traits with `help` and `where` entries. A script then rewrote the fixtures of `tests/fixtures/run`, `emit` and `check` that write `enum`, `trait`, `rust-type` or `rust-trait`, except those that build forms with `quote` or `template`, to `defenum`, `deftrait` and `deftype`, added `(use lib/definers defenum deftrait deftype)`, and compared each with its original through `rslisp emit`. These four substitutions are the whole rewrite, where `S` is a string literal and `H` is a head, a name or a list:

```text
(rust-trait NAME S   ->  (deftrait NAME (rust S)
(rust-type H S       ->  (deftype H (rust S)
(enum                ->  (defenum
(trait               ->  (deftrait
```

The outcomes of the 311 fixtures:

| Outcome | Fixtures |
| --- | ---: |
| The same Rust, byte for byte | 191 |
| The same set of error codes | 115 |
| A difference from the script, which did not copy a fixture's modules | 3 |
| A difference from the prototype's own shape checks | 2 |

So `method`, `structural`, `associated`, `where`, `with` and `help` pass through the clause unchanged.

| Mistake | Report |
| --- | --- |
| `(deftrait number i8 i16)` | a `deftrait` method is a name and a type, with the help to write `(rust "Path")` |
| `(deftrait number (rust "Copy") (area (fn ((ref self)) f64)))` | a `deftrait` with a `rust` clause lists its implementors here, with the help to write `(method name type)` |
| `(deftrait number i8 (rust "Copy"))` | the `rust` clause comes right after the name |
| `(deftrait shape (area (fn ((ref self)) f64)) (rust "Copy"))` | the `rust` clause comes right after the name |
| `(deftrait number (rust "Copy" "Clone") i8)` | a `rust` clause on `deftrait` holds one string |
| `(deftrait (number (clone)) (rust "Copy") i8)` | a Rust trait takes no supertraits |
| `(deftype meters f64)` | `deftype` names a Rust type, with the help to write the clause |

Without its own check, the second mistake reports only ``cannot find type `area` ``, which does not name `deftrait`. The check tells a method from an implementor by a list whose second item is a `fn` type, and no `rust-trait` in the repository holds such an implementor.

`rslisp doc` prints `(deftrait number)`, then the docstring, where a `rust-trait` prints the bare name `number` today, because a kernel form that no macro call wrote has no header. `rslisp fmt` puts the clause on its own line and fills a run of implementors, as it does for `rust-trait`:

```lisp
(deftrait from-string
  (rust "std::str::FromStr")
  (associated (parse-failure target) "Err")
  (where () i64 (parse-failure i64)))
```

### Build order

Each batch leaves `main` working and fits one pull request. Each batch updates `docs/language.md`, and `docs/diagnostics.md` where it changes a diagnostic.

**These batches can run beside RFC 0058's build.** Each changes the compiler or a line of the prelude that holds no flat `let`, so it does not touch what that build renames, except batch 4, which rebases over the 10 flat `let` lines of `prelude/number.rslisp` if RFC 0058 lands first.

1. **`match` arms (8).** The kernel change, `display-match`, and the retirement of `L0232`.
2. **Keyword patterns (10, 15).** The expander and type stage changes for struct and variant patterns, with their diagnostics.
3. **Keyword labels (11, 15).** Rule 2 leaves the constructor and `defn`, variant calls take labels, and 7 fixtures change.
4. **Values (7, 12, 16, 17, 18).** Clauses by trait land first in this batch, with their diagnostics. Then operators, constructors, `max` and `min` answer as values, the runtime gains `max` and `min`, the twins leave `export` while `neg` stays, and `L0306` follows.
5. **New definers (2, 3, 5).** `defenum`, `deftrait` and `deftype` join the prelude beside the kernel forms, with their checks, shapes, `types` output, the failed-pattern condition and the 14 helps, so no span of `main` ships an error that names a form the user did not write. Nothing migrates.

**These batches wait for RFC 0058's build.** That build rewrites 889 files and reflows them with `rslisp fmt`. Batches 6 and 7 edit `prelude/syntax.rslisp`, `macros`, `struct`, `text`, `test` and `assoc`, which hold 105 of the flat `let` lines it renames, and batch 6 alone changes 66 `take`, `drop` and `map-list` calls there. Batches 8 to 10 move the same fixtures.

6. **Library fixes (13, 14).** `eprint`, `prn` and `print-form`, `update`, data first in `code`, `doc-header` and `doc-body`, and the two doc examples.
7. **Internal modules (13).** The 21 helpers move, and `use` rejects the modules.
8. **`defenum` and `deftrait` migration (1, 2, 5).** A script rewrites 243 `enum`, 163 `trait` and 29 payload clauses in the prelude, the examples and the fixtures, then `rslisp fmt` runs. The script goes in the pull request description, so a reviewer reads it and the fixtures it cannot rewrite.
9. **`deftype` and Rust-backed `deftrait` migration (3).** The same for 70 `rust-type` and 80 `rust-trait` forms.
10. **Hiding (4).** The names leave `export`, the 170 fixture files import what they write or move to a prelude form, and `rslisp doc` gains the fallback. It needs batches 8 and 9.
11. **`let-match` (9).** The macro, its shape, and the 50 uses. It needs RFC 0058's `:bindings` marker. Batch 1 only lets its expansion drop the `do`.

### Open details

The build settles each of these. None changes a decision.

1. **`rust-type` and `rust-trait` in `export`.** Decision 3 says that both forms become `deftype` and `deftrait`, and decision 4's list does not name them. Batch 10 removes them from `export`, so a program has one spelling. They stay kernel forms, because the macros expand to them.
2. **How an operator answers as a value.** A `defrule` cannot test for a bare symbol, so each of the 13 names needs a `defmacro` by hand, or a small prelude helper, or a `defrule` clause for value position. A prototype `macro` that tests `symbol?` runs both `(tally 1 2)` and `(reduce (iter v) 0 tally)`.
3. **Several forms in a `display` arm.** `display` writes an arm's body as parts. Batch 1 chooses between treating every form of the arm as a part and following the `do` rule, and states it in `docs/language.md`.
4. **A keyword outside a pair.** `(->wrap :name)` on a one-field struct and `(->sorter "x" :name)` with `key` as the last field pass the reader by position today, through rule 4. "Always a label" makes each an error. No call in `prelude/`, `examples/` or `tests/fixtures/` writes either shape, for a struct or a keyword function that its own file defines.
5. **The internal modules.** One module cannot hold the 21 helpers: `prelude/macros` must import `note-docstring`, `docstring-before-parameters` and the other helpers `defmacro` needs, and three helpers of `prelude/syntax` use `template`, which `prelude/macros` defines, so one module would be `L0206`. The build uses two modules, because macro code keeps `template` and `map-list` that way, and only one helper changes. The first imports only `code` and holds the 8 helpers of `prelude/macros`, with `docstring-before-parameters` rewritten from `template` to `list` and `quote`. All six importers read it. The second imports `prelude/macros` and holds the 13 helpers of `prelude/syntax`, and only `prelude/syntax` and `prelude/struct` read it. The build still settles how the loader knows that an importing module belongs to the prelude, and whether a rejected `use` reads as "no module with this path" or names the module as internal.
6. **`update` on a non-Copy field.** The prototype passes a borrow of the field, as `(:name p)` reads one, so `(update a :name take-name)` with a `(fn (string) string)` reports `L0503`. Rust could move the field, as `name: take_name(a.name)`.
7. **`prn`'s separator.** Printed forms must read back, so several arguments need a space between them, as Clojure's `prn` writes one. `(prn 1 "a")` then prints `1 "a"`.
8. **`max` as a value.** `max` in value position stays the iterator function, as `str` stays one-argument. `(let top max)` fails at rustc today with `E0282`, because the closure's parameter is `_`.
9. **A hidden name's error.** `(new point (x 1))` reports `L0201` once `new` is hidden. Batch 10 decides whether `L0201` gains a note that names `(use kernel new)`, and whether `(doc new)` at the prompt follows `rslisp doc`.
10. **Formatter rules for arms.** RFC 0057, in pull request 345, keeps short arms on one line. Batch 1 checks those rules against an arm with several body forms.
11. **`eq`.** Decision 7 lists `eq` among the twins, and `eq` is also a trait, which `where` bounds use. `export` covers every namespace, so the RFC reads the decision as the function only: `prelude/number.rslisp` renames the function, and the prelude keeps exporting the trait. Each example that writes `(infer eq)` moves to another name.
12. **Mixed forms.** Decisions 10 and 15 say that patterns bind by name and that positional patterns stay. They do not say whether one form may hold both, as `(point a :y b)`. The RFC reads the two as separate forms and reports the mix as an error, in a struct pattern, a variant pattern and a variant call.
13. **A variant call by pairs.** Decision 15 says that the pairs build, and not whether a call may leave a field out or reorder the fields. The RFC reads a variant call as a `->name` call without defaults: every field, in any order. A left-out field reports `L0409`, which `(->point :x 1)` reports today.
14. **A keyword as a variant's value.** `(act/named :name)` on a variant with one named field of function type passes the reader by position today, as `(->wrap :name)` does. The RFC follows decision 11: on a variant with named fields a keyword is always a label, so this call is an error. On a positional variant, as in `(act/by :name)`, a keyword stays a value, because no field has that name.

### Earlier rules

Each RFC below keeps its reasoning, and its status line names the rule that this RFC replaces. An RFC that only spells a renamed form, such as RFC 0040's `rust-type` heads or RFC 0048's `structural` clause, keeps its rule under the new spelling, and its status does not change.

| Earlier rule | Replaced by |
| --- | --- |
| RFC 0038: its rule that a call with one argument per field is positional, for a keyword argument | decision 11 |
| RFC 0039: its rule 2, and its rejection of option B | decision 11 |
| RFC 0041, 0042, 0044, 0050 and 0052: the names `header` and `page-body` | decision 13 |
| RFC 0045: a clause of a `rust` form is a type and a template | decision 18 |
| RFC 0046: `add` and the other arithmetic names stay exported, and `>` has no value | decision 7 |
| RFC 0047: rules 3 and 4, `prn` as the printer of a result | decision 14 |
| RFC 0058: `let-pair` and `let-fields` keep their names | decision 9 |
| D5: a named variant's `struct` payload, the rejection of a prelude `defenum` and of a flat form, and construction and patterns as positional only | decisions 2, 5 and 15 |
| D15: `let-pair` | decision 9 |
| D25: `let-fields`, and struct patterns as positional only | decisions 9 and 10 |
| D32: `ok`, `err` and `some` can no longer be passed as function values | decision 7 |

## Rejected options and why

- **Keep both field spellings:** a user reads two spellings of one thing, and a uniform rule gives the better output.
- **Rename the kernel forms:** the prelude can express `defenum` and `deftrait`, so the kernel keeps its names.
- **`defrust-type` and `defrust-trait`:** the prototype shows that one `rust` clause covers both, so the fallback is not needed.
- **Keep the twins:** `+` as a value makes each twin a second name for one concept.
- **`let` with patterns:** `(some x)` and `(x i64)` have one shape, and a macro cannot tell a type from a variant.
- **`max-item` and `min-item`:** dispatch by argument count has a precedent in `reduce`, and needs no renames.
- **`{0}.max({1})` for values:** a function generic over `partial-ord` has no such method, and rustc reports `E0599`.
- **One runtime function for every type:** `crate::rt::max(1_i64, 2_i64)` does not read as hand-written Rust, and most calls have a concrete type that `std::cmp::max` takes.
- **Clauses by type alone:** the prelude cannot list a program's struct, so a struct that derives `ord` and a generic call would both take the default.
- **Clauses by type with `std::cmp::max` as the default:** a function generic over `partial-ord` fails at rustc with `E0277`, in emitted code that the user cannot change.
- **`std::cmp::max` for values:** it needs `Ord`, which the float types leave out.
- **A new trait for `ord` types and floats:** Rust rejects a blanket impl over `Ord` beside an impl for `f64` with `E0119`, and `L0418` rejects a bare type parameter as an implementor. A list of impls would leave out every type that derives `ord`, which `partial-ord` covers, because `ord` brings it.
- **Variants positional only:** structs take keyword labels, and a variant with named fields now does as structs do.
- **`neg` as a twin:** it is a different operation from `-`, because `-` as a value subtracts.
- **Accepting `(->point a b)` as a pattern:** the kernel would know a prelude naming convention. Building with `->point` and matching with `point` stay two names.

## Rule

[docs/language.md: Kernel forms](../docs/language.md#kernel-forms), [Standard types](../docs/language.md#standard-types), [Structs](../docs/language.md#structs), [Macros](../docs/language.md#macros), [Keyword parameters](../docs/language.md#keyword-parameters), [Docstrings](../docs/language.md#docstrings), [The `code` module](../docs/language.md#the-code-module), [Modules](../docs/language.md#modules), [Evaluation](../docs/language.md#evaluation), [Rust interop](../docs/language.md#rust-interop), [Names](../docs/language.md#names), [9. Enums and `match`](../docs/language.md#9-enums-and-match) and [10. User-defined traits](../docs/language.md#10-user-defined-traits). The build rewrites those sections.
