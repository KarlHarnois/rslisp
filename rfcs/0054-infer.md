# RFC 0054: `infer`, the inferred type of an expression

**Status:** Accepted
**Date:** 2026-11-05

## Problem

A user cannot ask the compiler what type it inferred. The project owner asked on 2026-11-05 for a form whose value is that type as text, so that `(infer (vec 1 2))` is `"(vec i64)"`. The owner chose the name `infer`: `type-of` is a run-time name in other languages, and `type` is the signature form.

Today a user gets the type only from a diagnostic, by forcing a mismatch, as Rust's `let () = x;` does. The probes below ran on a release build of `main` at `a0c6a844`, built with `nix develop -c cargo build --release`, on Linux, with rustc 1.95.0. Each probe is `rslisp eval` on a module that defines `point`, `shape`, `identity` and `add`.

| Probe | Output |
| --- | --- |
| `(the unit (vec 1 2))` | `L0401`: expected `unit`, found `(vec _)` |
| `(the unit 1)` | `L0401`: expected `unit`, found an integer |
| `(the unit (ref (->point 1 2)))` | `L0401`: expected `unit`, found `(ref point)` |
| `(the unit shape/empty)` | `L0401`: expected `unit`, found `shape` |
| `(let s (str "a")) (the unit (mut s))` | `L0401`: expected `unit`, found `(mut string)` |
| `(the unit identity)` | `L0401`: expected `unit`, found `(fn (_) _)` |
| `(the unit (some none))` | `L0401`: expected `unit`, found `(option (option _))` |
| `(the unit (:name u))` with a `string` field | `L0401`: expected `unit`, found `(ref string)` |
| `(the unit x)` in a function typed `(where ((a display)) (fn (a) unit))` | `L0401`: expected `unit`, found `a` |
| `(the unit (fn () (+ n 1)))` | `L0401`: expected `unit`, found `(fn () _)` |
| `(iter (vec 1 2))` | note: the value has type `(iterator (ref i64))`, which does not implement `show` |
| `(fn (n) (+ n 1))` | note: the value has type `(fn (i64) i64)`, which does not implement `show` |
| `none` | `L0407`: cannot infer the type `(option _)` of this value |
| `identity` | `L0407`: cannot infer the type `(fn (_) _)` of this value |
| `(type-name (vec 1 2))`, with `type-name` a `rust` template over `std::any::type_name_of_val` | `"alloc::vec::Vec<i64>"` |
| `(type-name (->point 1 2))` | `"m::Point"` |

Three facts follow:

- **The checker already spells types in the language's syntax.** `TypeDisplay` in `src/types/display.rs` writes every diagnostic's type and `eval`'s note, and `TypeDisplay::scheme` writes a `where` form. The text is the syntax a signature writes.
- **A mismatch shows the type before literal defaulting.** `(vec 1 2)` shows `(vec _)` and `1` shows "an integer", because the checker reports the mismatch at the unification, before literals default. The trick also stops the program.
- **Rust's `type_name` spells Rust.** It names `alloc::vec::Vec<i64>` and the emitted `m::Point`, and its contents are "not specified" and "may change between versions of the compiler" ([`std::any::type_name`](https://doc.rust-lang.org/std/any/fn.type_name.html#note)). It also evaluates its argument.

### Why the prelude cannot express `infer`

A prelude macro runs during expansion, before the checker infers any type, so a macro sees forms and never a type. A prelude function runs in the emitted program, where the only type text is Rust's `type_name`, which spells Rust as the probes show, and runs its argument. To turn Rust's text back into the language's, the prelude would restate in Rust every name that the prelude binds, which `AGENTS.md` forbids, and it still could not name a type variable such as `a`, because Rust has erased it into each instantiation. Only the checker knows the inferred type, as only the checker knows a collection's type for `assoc-at`. So `infer` is a kernel form, and the build records that reason in `docs/language.md` under Kernel forms.

## Prior art

| Tool | Form | Runs the expression | Text |
| --- | --- | --- | --- |
| GHCi | `:type e`, short `:t` | No | `e :: T`. `:type +d` "instantiates all the forall quantifiers, solves constraints, defaults, and generalises." |
| OCaml toplevel | every phrase | Yes | The toplevel "prints the inferred type and result value, if any", as `- : int list = [1; 2]`. Editors ask Merlin for the type at a position. |
| Rust | `std::any::type_name::<T>()`, `type_name_of_val(&x)` | `type_name_of_val` does | Rust's text, "intended for diagnostic use", unspecified and unstable between compilers. `let () = x;` reads the type from rustc's mismatch error. |
| TypeScript | hover in the editor, `typeof x` in a type | No | `typeof` is a type, not text, and it is "only legal ... on identifiers (i.e. variable names) or their properties". |
| Zig | `@TypeOf(e)`, `@typeName(T)` | Evaluated with "no runtime side-effects" | `@typeName` gives "the string representation of a type, as an array", with parent namespaces joined by dots. |
| Julia | `typeof(x)` | Yes | "Get the concrete type of `x`", the run-time type of the value. |
| Lean 4 | `#check e` | No | `e : T`. A command, not a term: `#eval` evaluates. |

The closest designs are Zig's `@typeName(@TypeOf(e))`, a compile-time string from an expression whose effects never run, and GHCi's `:type`, which never runs the expression and generalizes what it does not know. Julia and Rust's `type_name_of_val` run the expression and report a run-time type, which a language with type erasure into generics cannot match.

Sources: [GHCi commands](https://downloads.haskell.org/ghc/latest/docs/users_guide/ghci.html#ghci-commands), [OCaml toplevel](https://ocaml.org/manual/latest/toplevel.html), [Merlin](https://github.com/ocaml/merlin), [`std::any::type_name`](https://doc.rust-lang.org/std/any/fn.type_name.html), [`std::any::type_name_of_val`](https://doc.rust-lang.org/std/any/fn.type_name_of_val.html), [TypeScript `typeof` types](https://www.typescriptlang.org/docs/handbook/2/typeof-types.html#limitations), [Zig `@TypeOf`](https://ziglang.org/documentation/master/#TypeOf) and [`@typeName`](https://ziglang.org/documentation/master/#typeName), [Julia `typeof`](https://docs.julialang.org/en/v1/base/base/#Core.typeof), [Lean 4 `#check`](https://lean-lang.org/theorem_proving_in_lean4/dependent_type_theory.html#simple-type-theory).

## Options

Every option makes `infer` a kernel form, `(infer expression)`, whose type is `(ref str)`. Its one position is an expression that the checker checks and the program never runs. The options differ in what the text says.

### The form

```lisp
(defn main ()
  (println (infer (vec 1 2))))
```

```rust
fn main() {
    crate::rt::restore_default_sigpipe();
    println!("{}", "(vec i64)")
}
```

- **Stages.** The expander resolves `infer` as a kernel form with one expression position. The checker infers the operand in its place, so the operand sees the enclosing scope, and gives the form the type `(ref str)`. After the enclosing definition's inference, pinning and literal defaulting finish, the checker writes the operand's type with `TypeDisplay` and replaces the form with a string literal in the typed IR. The ownership pass and the emitter never see the operand.
- **Cost.** The emitted Rust is one `&'static str` literal: no allocation, no `Show` code, and no code for the operand. `(vec 1 2)` above builds no vector. `println` folds only a written string literal into its format string, so the literal stays a `{}` argument, as the emitted Rust shows. The checker formats one string per `infer`, which is small next to the front end's 20 ms for RFC 0047's 38-definition session.
- **Bindings.** A binding whose only use is an `infer` operand still emits, as an unused binding does today: `(let v (vec)) (if false (push! v 1) (do))` emits `let mut v: Vec<i64> = Vec::new();`, and `--cap-lints allow` hides rustc's unused-variable warning.

### Question 1: the type text

The text is the type as a `type` signature writes it, so it pastes into a signature. It reuses `TypeDisplay`, which writes every type in today's diagnostics:

| Expression | `(infer expression)` |
| --- | --- |
| `true`, `(char "a")`, `(do)` | `"bool"`, `"char"`, `"unit"` |
| `"hi"`, `(str "hi")` | `"(ref str)"`, `"string"` |
| `(vec 1 2)`, `(pair 1 "a")` | `"(vec i64)"`, `"(pair i64 (ref str))"` |
| `(some 2.5)`, `(sorted-map "a" 1)` | `"(option f64)"`, `"(sorted-map (ref str) i64)"` |
| `(->point 1 2)`, `shape/empty` | `"point"`, `"shape"` |
| a value of `(struct (holder a) (item a))` holding `1` | `"(holder i64)"` |
| `(ref p)`, `(mut s)` | `"(ref point)"`, `"(mut string)"` |
| `(:name u)` for a `string` field | `"(ref string)"`, because a field read borrows, as the Ownership section says |
| `(iter (vec 1 2))` | `"(iterator (ref i64))"` |
| `(fn (n) (+ n 1))` | `"(fn (i64) i64)"` |
| a parameter of type `(ref 'a str)` | `"(ref 'a str)"`: a written lifetime shows, an elided one does not |

Two kinds of type need a choice:

- **A closure's kind.** The ownership pass decides whether a closure is `fn-mut` or `fn-once`, after the checker. The text writes `fn` for every function type, which in a signature leaves the kind to inference, so the text still pastes. A text that names the kind would have to wait for the ownership pass, and would make the form's value depend on a later stage than the one that types it.
- **A type the checker has not resolved.** Options T1 and T2 below.

#### T1: each unresolved type is `_`

```text
> (infer none)
"(option _)"
> (infer identity)
"(fn (_) _)"
```

`_` is the signature syntax for an inferred type, so the text still pastes. It matches `L0407` and `L0401` today. It loses the link between positions: `(fn (_) _)` does not say that `identity` returns what it takes.

#### T2: unresolved types that belong only to the operand are generalized

The checker generalizes the operand's type over each type variable that appears in no binding of the enclosing scope, with the bounds those variables carry, and writes the result with `TypeDisplay::scheme`. The operand's generalized variables take the next names that the enclosing scheme does not use, so they never collide with the enclosing function's own variables. A variable that a local binding shares is not generalized. The text is written after the definition's inference, so it shows the type the body fixes. A variable the body never fixes is `L0407` at its binding, as without `infer`. A literal's variable is never generalized: it defaults, as Question 3 says.

```text
> (infer none)
"(where (a) (option a))"
> (infer identity)
"(where (a) (fn (a) a))"
> (do (let v (vec)) (push! v (char "x")) (infer v))
"(vec char)"
```

Inside a generic function, the operand's variable takes the next free name:

```lisp
(defn wrap (x) (println (infer (pair x none))) (some x))   ; prints (where (b) (pair a (option b)))
```

`x` is `a`, the function's own variable under G1, and `none`'s variable becomes `b`. The probe `(do (let v (vec)) (push! v (char "x")) (the unit v))` reports `found (vec char)`, the type the text shows. Each prompt input is its own form, so `v` must be bound in the same input: `(let v (vec))` entered alone is `L0407`.

This is GHCi's `:type`, which shows `id :: a -> a`. The text pastes into a `type` signature whole. The cost is one rule more than T1: the same generalization a top-level `def` gets, applied to the operand.

### Question 2: a parameter inside a generic function

```lisp
(defn wrap (x) (println (infer x)) (some x))
```

#### G1: the type variable

`(infer x)` is `"a"`, the name the function's scheme gives the parameter, which is the name `L0401`'s help already writes in a suggested signature. A written signature keeps its own name, as the probe's `found a` shows. Every call prints the same text.

- One string per function, so the emitted Rust stays one `&'static str`. The operand adds no type parameter, though its bounds count, as Question 5 says.
- The text does not change when a second caller appears, even though the Types section emits a function used at one type with that type in place of its parameter. The text reports the function's own inferred type, not what its callers give it.
- `"a"` pastes only inside that function's `where`.

#### G2: one string per instantiation

`(wrap 1)` prints `i64` and `(wrap "a")` prints `(ref str)`.

- The emitted function is one Rust generic, so the string must come from the type argument at run time. The compiler would add a hidden bound, such as `A: crate::rt::TypeText`, to `wrap`'s Rust signature, and emit an impl of that trait for every type that reaches it, each spelled in the language's syntax. That is `type_name` rebuilt with the compiler restating every prelude type's name in Rust.
- A form meant for reading types changes the emitted signature of the function that holds it, and a bound spreads to every generic caller.
- A function used at one type would print the concrete type, and adding a second caller would not change it, but a caller that is itself generic would print its own variable, so the text depends on the call chain.

### Question 3: an integer literal before defaulting

`(infer 1)` is `"i64"`, and `(infer 1.5)` is `"f64"`. The text is written after the enclosing definition's inference finishes, so it follows the Literals rules as the emitted Rust does:

```lisp
(let x 1)
(println (infer x))   ; prints u8
(the u8 x)
```

A pin anywhere in the definition, or from a caller under the Literals pin rules, reaches the text, and a literal that stays open defaults to `i64` or `f64`. A literal under a signature's type parameter shows that parameter, as `x` in `(type increment (where ((n number)) (fn (n) n)))` shows `"n"`.

The alternative, the type at the point of the form before defaulting, has no syntax: `"an integer"` is not a type, and rustc's `{integer}` uses characters the reader rejects. GHCi's `:t 1` shows a class-constrained variable instead, but a literal never makes a type generic here, as the Literals section says.

### Question 4: an expression that does not check

`(infer expression)` reports the operand's type errors as any expression does, and the program does not compile. A text that names the failure, such as `"(error L0401)"`, would let a program with a type error compile and run, and would hide the label and help that point at the cause. An unresolved type is not an error inside `infer`, because the operand is never emitted: under T1 it is `_`, and under T2 it is generalized. So `(infer none)` checks, where `none` alone at the prompt is `L0407`.

### Question 5: ownership

- **The text shows types, not passing modes.** Type inference ignores ownership except where it is written, so `(ref ...)` and `(mut ...)` appear where the source writes them, where a field read borrows, and where a prelude function's signature returns a reference, as `iter` does. A parameter's mode, which the ownership pass picks after the checker, does not appear: in `(defn total (v) (println (infer v)) (sum v))`, `(infer v)` is `"(vec i64)"`, though `total` takes `&[i64]`. The text is the type a signature writes for that parameter, and pasting it into the signature keeps the program's meaning.
- **`infer` neither borrows nor moves its operand.** The operand never runs, so the ownership pass skips it: `(infer s)` after `s` moved is allowed, and `(infer (consume s))` does not move `s`. `(infer (println "x"))` prints nothing and is `"unit"`.
- **The operand's constraints count, bounds included.** The operand is checked as code, as a branch that never runs, so `(infer (push! v 1))` fixes `v`'s element type though the push never runs, as an `if` branch that never runs fixes types in Rust. A bound the operand needs reaches the enclosing function's signature too: `(defn wrap (x) (println (infer (str x))) (some x))` generalizes to `(where ((a display)) (fn (a) (option a)))`, its Rust signature gains `A: Display`, and `(wrap (vec 1))` is `L0404`, as it is today for the same body with `(str x)` outside `infer`. The probe `(defn wrap (x) (println (str x)) (some x))` emits `fn wrap<A: std::fmt::Display>(x: A) -> Option<A>`, where `(defn wrap (x) (some x))` emits `fn wrap<A>(x: A) -> Option<A>`, and `(wrap (vec 1))` against it reports `` `(vec a)` does not implement `display` ``. Discharging the operand's obligations against a copy and dropping them would keep the signature, but then the operand would not check as the same code checks elsewhere.

### Question 6: the prompt

`infer` returns a `(ref str)`, and RFC 0048 prints a string as a string literal:

```text
> (infer (vec 1 2))
"(vec i64)"
> (infer (fn (n) (+ n 1)))
"(fn (i64) i64)"
```

The quotes are the only cost. A type text never holds a `"` or a newline, so it prints on one line, and RFC 0050 colors it as one string. A `\` in a type name prints as `\\`, by RFC 0048: with `(defstruct a\b :x i64)`, `(the unit (->a\b 1))` reports `found a\b` today, so `(infer (->a\b 1))` prints `"a\\b"`, as the string `"a\\b"` prints at the prompt. RFC 0052 made `doc` print because a page's newlines printed as `\n`, which does not happen here.

A shorter way to ask:

- **`:t`, as GHCi's.** `:t` is a keyword, and `(:t x)` already reads field `t` of `x`. A prompt command outside the language would make the prompt treat one input by its head, which RFC 0052 rejected as option E.
- **A prelude name such as `t`.** It costs the user a common one-letter name in every module for four characters at the prompt.
- **The prompt shows every result's type, as OCaml's toplevel does.** `eval` already knows each form's type. It changes every result line and `eval --json`'s `value`, which is a separate design.

### Edge cases

- **An unbound name in the operand** is `L0201`, as for any expression: `(infer (nope 1))` reports `` cannot find `nope` in this scope ``.
- **A macro call as the operand** is expanded before the checker sees it, so `(infer (->point 1 2))` is `"point"`.
- **`infer` in a macro's expansion** types its operand in the expansion's scope, and the text writes the names of the bindings that scope resolves.
- **`(def t (infer x))`** is a definition of a `(ref str)`, at the prompt as in a file, and `t` then prints `"..."` as any string `def` does.
- **`eval --json`** carries the text as any string value: `(infer (vec 1 2))` gives `{"value":"\"(vec i64)\"","form":1}`, as `"(vec i64)"` does today.
- **`(infer (infer x))`** is `"(ref str)"`, the type of the inner form.

## Recommendation

A kernel form `(infer expression)` whose value is a string literal of the operand's type, with these answers:

1. **The type text** is the signature syntax that `TypeDisplay` writes, with `fn` for every function kind and T2 for unresolved types: `(infer identity)` is `"(where (a) (fn (a) a))"`. T2 gives the most useful text, and every text it writes pastes into a `type` signature.
2. **A generic parameter** gives its type variable, G1: `"a"`. One string per function keeps the emitted Rust one literal and adds no type parameter.
3. **A literal** follows defaulting and pinning: `(infer 1)` is `"i64"`, and `"u8"` where the definition pins it.
4. **A failing operand** reports its errors. `infer` adds none of its own, and an unresolved type is never an error.
5. **Ownership:** the text shows written and inferred references, not passing modes, and `infer` neither borrows nor moves nor runs its operand.
6. **The prompt** prints `"(vec i64)"`, quoted, by RFC 0048. No shorthand.
7. **The emitted Rust** is the string literal alone, with no code for the operand. A bound the operand needs still reaches the enclosing function's signature.

## Decision

The project owner accepted every recommendation on 2026-11-05:

1. **T2:** unresolved types that belong only to the operand are generalized, so `(infer identity)` is `"(where (a) (fn (a) a))"`.
2. **G1:** a generic parameter gives its type variable, `"a"` on every call.
3. **Closure kinds:** the text writes `fn` for every function type, never `fn-mut` or `fn-once`.
4. **The operand's constraints:** the operand never runs, but its constraints count, so `(infer (push! v 1))` fixes `v`. The owner decided on 2026-11-05 that bounds from the operand count too: the operand is checked as code, as a branch that never runs, so an `infer` can add a bound to its function's signature, as `(defn wrap (x) (println (infer (str x))) (some x))` gains `(a display)`.
5. **The prompt:** the prompt prints the quoted string. The language adds no shorthand and no type line on every result.
6. **Names:** two types of one name from two modules print alike, as diagnostics do.

## Rejected options and why

- **T1, `_` for every unresolved type:** `(fn (_) _)` loses the link between positions that `(where (a) (fn (a) a))` keeps.
- **Discharging the operand's bounds against a copy:** the operand would check differently from the same code outside `infer`.
- **G2, one text per instantiation:** it adds a hidden bound to the emitted function's Rust signature and makes the compiler restate every prelude type's name in Rust.
- **`fn-mut` and `fn-once` in the text:** the ownership pass picks the kind after the checker, and a plain `fn` already pastes into a signature.
- **A type line on every prompt result:** it changes every result line and `eval --json`'s `value`, which is a separate design.
- **A prelude macro:** a macro runs before the checker and never sees a type.
- **A prelude function over Rust's `type_name`:** it spells Rust's names, such as `alloc::vec::Vec<i64>`, runs its argument, and cannot name a type variable.
- **A text that names a type error:** a program with a type error would compile and hide the cause.
- **The type before literal defaulting:** "an integer" has no type syntax, and `{integer}` uses characters the reader rejects.
- **Passing modes in the text:** the mode is a calling convention that the ownership pass picks after the checker, and pasting it into a signature changes the program.
- **`:t` or another reader character:** the reader treats only `(`, `)`, `"`, `;` and whitespace specially, and `:t` already reads a field.
- **The names `type-of` and `type`:** the owner rejected `type-of` as a run-time name in other languages, and `type` is the signature form.

## Rule

[docs/language.md: Kernel forms](../docs/language.md#kernel-forms). The build that lands the decision adds the `infer` row to the kernel forms table, with its position, an expression that is checked and never run, and states there why the prelude cannot express it: only the checker knows the inferred type. It states the type text under Types and the prompt's output under The prompt.
