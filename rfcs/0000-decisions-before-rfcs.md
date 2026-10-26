# Decisions before RFCs

This file records the decisions the project owner made before RFCs existed. Each entry gives the decision, the rejected options, the principle at stake and the section of `docs/language.md` that states the rule today. The entries are `D1` to `D37` in order.

## D1. How a value meets an expected type

- **Decision:** Wherever a value meets an expected type, the compiler adds one borrow, removes borrows, or copies a Copy value of 16 bytes or less out of a borrow. It never clones, a written `ref`, `mut` or `deref` turns the rule off, and mutating functions end in `!`.
- **Rejected:** Explicit `deref` and `as-str` everywhere; Rust's coercion rules as written; ownership erased from the types.
- **Why:** Ownership stays in the types but users rarely write it, and the emitted Rust stays hand-written Rust with no runtime cost.
- **Rule:** [8.1 References at call sites](../docs/language.md#81-references-at-call-sites)

## D2. How a struct gets traits before milestone 10

- **Decision:** A struct gets each structural trait its field types support: `clone`, `debug`, `partial-eq`, `eq`, `hash`, and `copy` when nothing changes it in place. A written `(derive person ...)` is exact and adds, removes or pins traits, and `prn` prints the data form.
- **Rejected:** A `derive` that only adds, plus a separate way to remove traits; `defstruct` defaults with opt-in `copy`, `eq` and `hash`; waiting for milestone 10.
- **Why:** Traits are inferred by default and written where control is wanted, and a struct that something changes is never copied, so a change cannot land on a copy.
- **Rule:** [8.2 Struct traits](../docs/language.md#82-struct-traits)

## D3. How macro code computes

- **Decision:** Keep the phases apart. `defrule` is the documented default for macros, the prelude exports its list helpers for macro code, L0306 names the replacement from `code`, and `same-binding?` is fixed; a macro that computes still writes `number-add`.
- **Rejected:** Compile-time twins with narrow semantics and an agreement test; phase-scoped imports as in Racket.
- **Why:** The compiler stays small with one implementation per name, and after milestone 9 no macro in the dogfood library needs compile-time arithmetic.
- **Rule:** [Macros](../docs/language.md#macros)

## D4. How a program loops

- **Decision:** A self-call in tail position compiles to a Rust `loop`, and a written `recur` compiles to the same loop. `recur` is an error outside tail position and works in lambdas, so the prelude's `loop` is a `defrule`; inference comes first, then `recur`.
- **Rejected:** Only `recur`, as in Clojure; only inference; a `while` with assignment; trampolines for mutual recursion, which allocate.
- **Why:** Rust has no guaranteed tail calls, so loops emit as `loop` and mutual recursion stays recursion, and one kernel form, `recur`, covers it.
- **Rule:** [8.4 Loops](../docs/language.md#84-loops)

## D5. How a program defines an enum

- **Decision:** Variants take positional payloads by default: `(circle f64)`, `(rect f64 f64)`, and a bare `point` for no payload. A variant that wants names holds an inline `struct` payload, as `(rect (struct (width f64) (height f64)))`, and construction and patterns stay positional for every variant.
- **Rejected:** Named fields in every variant; a union of separate structs; a prelude `defenum`; a flat form with separators.
- **Why:** Few rules keep the syntax simple, since after a variant's name comes types or one `struct` body, and Rust uses tuple and struct variants the same way.
- **Rule:** [9. Enums and `match`](../docs/language.md#9-enums-and-match)

## D6. Where a type definition names its parameters

- **Decision:** A type definition names its parameters on its name, for `enum`, `struct` and `rust-type`: `(enum (option a) (some a) none)`, `(struct (pair a) (left a) (right a))`, and a bound as `(a ord)`. `where` stays for signatures and trait entries.
- **Rejected:** Keep `(where (a) ...)` around the body, which nests every variant one level deeper.
- **Why:** A definition reads like its use, as in Coalton, Hackett and Carp, and Rust attaches type parameters to the type's name.
- **Rule:** [Types](../docs/language.md#types)

## D7. How a program names a variant

- **Decision:** Construction is qualified, as `(shape/circle 1.0)` and `shape/point`. Patterns use short names, as `(match s ((circle r) ...) (point 0.0))`, and the enum is inferred from the matched value's type or from the patterns when that type is unknown; a qualified pattern is always allowed and is required only when two visible enums fit.
- **Rejected:** Bare variant names everywhere, which silently shadow prelude names such as `add` and `print`; qualified names everywhere; a `match` that names its enum, which breaks down for nested patterns inside `option`.
- **Why:** The enum is inferred and a qualified name is the exact form, so construction never clashes, as Swift and OCaml resolve patterns from the type.
- **Rule:** [9. Enums and `match`](../docs/language.md#9-enums-and-match)

## D8. What `option` and `result` become

- **Decision:** `option` and `result` are prelude enums with a `rust` clause, as `(enum (option a) (rust "Option") (some a) none)`. They emit Rust's own `Option` and `Result`, `match` destructures them, the short constructors `(some 3)`, `none`, `(ok v)` and `(err e)` stay prelude functions beside `option/some`, and a `derive` on them is an error.
- **Rejected:** Qualified construction only, as `(option/some 3)`; names the compiler knows, an exception to how enums work.
- **Why:** The emitted types stay `Option` and `Result`, and the short constructors are prelude functions that work as function values, as `make-point` does.
- **Rule:** [9. Enums and `match`](../docs/language.md#9-enums-and-match); the constructor rule is superseded by [D32](#d32-template-hygiene-and-result-variants)

## D9. How `match` and its patterns work

- **Decision:** `match` is a kernel form, as `(match s ((circle r) ...) (point 0.0) (_ ...))`. Patterns are a variant with patterns inside, `_`, a literal or a bare name, and they nest; a bare name that is a variant of the matched enum is that variant, and any other binds.
- **Rejected:** A prelude macro over `if` and accessors, which cannot bind payloads without clones or check coverage; a `case` over variant names only.
- **Why:** The emitted `match` is the hand-written one, and only the compiler knows the variant set, so coverage is checked before rustc.
- **Rule:** [9. Enums and `match`](../docs/language.md#9-enums-and-match)

## D10. How a pattern binds through a reference

- **Decision:** Matching a `(ref t)` binds each name as a `ref`, and a `(mut t)` binds each as a `mut`, as Rust's default binding modes do. Uses follow `D1`, so a small Copy payload is copied out and nothing is cloned.
- **Rejected:** Explicit `(deref s)` and `ref` patterns, as in Rust 2015; binding by value, which clones heap payloads.
- **Why:** The binding is inferred unless written, and `D1`'s one rule applies to every value that meets a type, with no hidden clone.
- **Rule:** [9. Enums and `match`](../docs/language.md#9-enums-and-match)

## D11. How an error propagates

- **Decision:** `(try expression)` is a kernel form that emits `expression?`, with no `?` alias. A `result` needs a function that returns a `result` with the same error type, an `option` needs one that returns an `option`, and inference gives the function that return type; the prelude gains `ok-or`, `let-try` and `try->`, and `map-err` converts errors until milestone 10.
- **Rejected:** A `return` form with `try` as a prelude macro, a second way to leave a function; a tail-only macro, too weak for the stack machine and the calculator.
- **Why:** It compiles to exactly Rust's `?`, one form exists because the prelude cannot return early, and the reader stays as it is.
- **Rule:** [9.1 Propagating errors with `try`](../docs/language.md#91-propagating-errors-with-try)

## D12. How an enum gets traits

- **Decision:** As `D2` for structs, with payloads in place of fields: each structural trait that all payloads support is inferred, `copy` is inferred when nothing changes the enum in place, and a written `derive` is exact. An order follows the declared variant order, and errors name the variant whose payload blocks a trait; `default` waits for syntax that marks a variant.
- **Rejected:** No inference, so every enum writes its list; no order, so a `priority` enum cannot be sorted.
- **Why:** Traits are inferred unless written, with one rule for structs and enums.
- **Rule:** [9. Enums and `match`](../docs/language.md#9-enums-and-match)

## D13. How a recursive enum holds itself

- **Decision:** Inference boxes each position that holds the enum by value, so `(enum (tree a) (node (tree a) a (tree a)) leaf)` emits `Box<Tree<A>>` and construction and patterns stay plain. A written `(box ...)` is the exact form, `signatures` prints the inferred boxes, and `AGENTS.md` gains the exception for the `Box` a recursive enum position requires.
- **Rejected:** A written `box` everywhere; a written `box` once in the declaration.
- **Why:** Values are boxed implicitly as in functional programming, and the allocation is one no program can avoid, so the compiler hides no choice.
- **Rule:** [9. Enums and `match`](../docs/language.md#9-enums-and-match)

## D14. Which parts of milestone 9 are kernel forms

- **Decision:** Three forms are kernel forms: `enum`, `match` and `try`. The prelude holds `option`, `result`, their short constructors, `let-try`, `try->` and `ok-or`, and `docs/language.md` records why the prelude cannot express each kernel form.
- **Rejected:** No new kernel forms, with tag structs and macros, which cannot keep Rust's `Option` or check coverage.
- **Why:** Each kernel form is something only the compiler can do, which keeps the compiler small and the prelude large.
- **Rule:** [Kernel forms](../docs/language.md#kernel-forms)

## D15. The pair type, for `enumerate` and `zip`

- **Decision:** `(enum (pair a b) (rust "({0}, {1})") (pair a b))` makes a `rust` clause hold a type template, so a one-variant enum with one emits as a Rust tuple. Build with the prelude function `(pair 1 "a")`, match with `(pair index line)`, the same word, as `(some x)` does for `option`, and destructure with `let-pair`; pairs get `partial-ord` and `ord`, and `enumerate` emits `{0}.enumerate()`.
- **Rejected:** A prelude enum `Pair::Of` with no compiler change, which converts every tuple from `enumerate` and `zip` and names a prelude module path in a template.
- **Why:** The emitted Rust uses Rust's own tuples, and the change extends an existing clause instead of adding a kernel form.
- **Rule:** [9. Enums and `match`](../docs/language.md#9-enums-and-match)

## D16. How `parse` names its target and its error type

- **Decision:** A `rust` template names a type parameter of its `where` by name, as `{target}` in `"{0}.parse::<{target}>()"`. A `rust-type` lists a Rust form per argument type after its fallback, as `(i64 "std::num::ParseIntError")`, so the prelude states each error type, and programs keep writing `(parse text)` and `(parse-error i64)`.
- **Rejected:** A table of error types in the compiler, which restates prelude facts in Rust; the turbofish alone, which keeps `<i64 as FromStr>::Err`.
- **Why:** The facts live in the prelude, and two existing forms grow instead of a new kernel form, which keeps the compiler small.
- **Rule:** [Rust interop](../docs/language.md#rust-interop)

## D17. How the emitted Rust is formatted

- **Decision:** `rslisp emit` and `rslisp build` format the emitted Rust with `rustfmt`, in the CLI layer, so the compiler stages stay pure and gain no dependency. Token alignment rebuilds the span map so rustc errors still point at the source, and without `rustfmt` the output stays unformatted and the CLI says so once.
- **Rejected:** `prettyplease` in the emit stage, which mangles macro arguments; the emitter's own layout, which would be maintained forever; deferring.
- **Why:** The output is what `cargo fmt` produces, including inside macros, at about 17 ms per run, and `rustfmt` ships with a standard Rust toolchain.
- **Rule:** [Formatting](../docs/language.md#formatting)

## D18. How a program defines a trait

- **Decision:** `(trait shape (area (fn ((ref self)) f64)) (scale! (fn ((mut self) f64) unit)))` binds `self` as a type variable of the trait, and each method is an ordinary function type whose first parameter states the receiver's ownership. The emitted Rust uses real receivers, as `fn area(&self) -> f64`, and calls emit `shape.area()`.
- **Rejected:** A Clojure-style untyped first argument, `(area (this) f64)`, which needs a receiver typing rule; an implicit `self` value that borrows silently.
- **Why:** `self` works like a `where` binder and ownership is written as everywhere else, and the emitted Rust has real method receivers.
- **Rule:** [10. User-defined traits](../docs/language.md#10-user-defined-traits)

## D19. How a program implements a trait

- **Decision:** `(impl shape circle (area (c) body) (scale! (c factor) body))` writes each method like `defn`, and its parameter types come from the trait; the first parameter emits as Rust's `self`. A program implements a trait for its own structs and enums and for prelude types such as `i64`, and a missing, extra or undeclared method is an error.
- **Rejected:** Methods as function values, `(area (fn (c) body))`, which adds a wrapper with no information; named functions linked by a separate implement form, which gives each method two names.
- **Why:** An impl reads as a group of `defn` forms, as in Clojure's `extend-protocol`, so the code reads functionally.
- **Rule:** [10. User-defined traits](../docs/language.md#10-user-defined-traits)

## D20. How a program calls a method, and how inference treats it

- **Decision:** A method is an ordinary function, so `(area c)` is a call and `(map shapes area)` passes it as a value. A call on an unknown type adds the trait's bound, as `<` adds `ord`, and a written signature stays exact; a method name is a top-level name, so two traits in one module cannot share one, and `use` picks between modules.
- **Rejected:** Always qualified, `(shape/area c)`, which lengthens every call; both forms, with an ambiguity error and two spellings.
- **Why:** Methods compose as functions, as in functional code, and inference reuses the existing bound mechanism.
- **Rule:** [10. User-defined traits](../docs/language.md#10-user-defined-traits)

## D21. Whether a program implements Rust's own traits

- **Decision:** Not in milestone 10: `trait` and `impl` cover a program's own traits, and Rust's traits stay reachable through `rust-trait` and `derive`.
- **Rejected:** Allow `impl` of Rust traits now, which exposes `Formatter` and `fmt::Result` to programs or needs that wrapper designed in a hurry.
- **Why:** Rust's low-level details stay hidden, so printing a user type gets its own design instead of exposing `Formatter`.
- **Rule:** [10. User-defined traits](../docs/language.md#10-user-defined-traits)

## D22. Values of mixed types behind one trait

- **Decision:** Not in milestone 10: generic code works over one type at a time and allocates nothing. Later, when a program needs mixed collections, a written `(box shape)` is `Box<dyn Shape>`, so the allocation stays visible and `D18`'s real receivers keep that open.
- **Rejected:** Boxing automatically where mixed types meet a trait, a hidden allocation that `AGENTS.md` forbids.
- **Why:** No allocation happens that the source does not write.
- **Rule:** [10. User-defined traits](../docs/language.md#10-user-defined-traits)

## D23. Which parts of milestone 10 are kernel forms

- **Decision:** Two forms are kernel forms: `trait` and `impl`. A method call needs no kernel form, because each method is an ordinary generic function and the existing call, inference and emission paths handle it.
- **Rejected:** Prelude macros over `rust-trait` and `rust`, which cannot emit Rust items or register implementors; method bodies inside `derive`.
- **Why:** Each kernel form is something only the compiler can do, which keeps the compiler small and the prelude large.
- **Rule:** [Kernel forms](../docs/language.md#kernel-forms)

## D24. How a program changes a field, an element or a local

- **Decision:** Both forms exist: `set!` changes a place in place, as `(set! (field cell alive) true)` emits `cell.alive = true` and `(set! (at (at grid y) x) alive)` emits `grid[y][x] = alive`, with `mut` and `&mut` inferred and a non-place an error; `assoc` returns a changed value and consumes the old one, as `(assoc cell (alive true))` emits `Cell { alive: true, ..cell }`, and neither form copies.
- **Rejected:** Only `set!`, which reads imperatively; only `assoc`, which cannot change a value another binding still holds; a kernel `set!`, which the prelude makes unnecessary; `at!` as a separate element accessor.
- **Why:** The code reads functionally with `assoc` and in place with `set!`, and neither form copies.
- **Rule:** [Ownership](../docs/language.md#ownership)

## D25. How a program takes a struct apart

- **Decision:** Struct patterns work in `match`, positionally in field order with `_` to skip, as variant patterns do: `((parsed node _) (tree/branch node))`. An owned struct moves its fields out, a borrowed one binds borrows, and the prelude adds `let-fields` over `match`, as `let-pair` is.
- **Rejected:** A `take` form over a field, a new kernel form for one field at a time; a field read that moves when an owned value is expected, which makes every read depend on context.
- **Why:** The existing `match` and its positional patterns grow to structs, with no new kernel form.
- **Rule:** [9. Enums and `match`](../docs/language.md#9-enums-and-match)

## D26. Closures that consume, change or read their captures

- **Decision:** A closure's body decides whether it moves, changes or only reads its captures, and a function's use of its parameter decides whether it calls it once, many times, or while changing it; the emitted Rust uses `impl FnOnce`, `impl FnMut` or `impl Fn`. A signature MAY write `(fn-mut (a) b)` or `(fn-once (a) b)` beside `(fn (a) b)`, and the prelude's `rust` templates write them where Rust cannot infer the kind.
- **Rejected:** A written kind such as `(once (fn ...))` in every higher-order signature; inference from the function's side only, which is imprecise for returned closures.
- **Why:** The kind is inferred unless written, as for ownership and bounds, with no new syntax.
- **Rule:** [Ownership](../docs/language.md#ownership)

## D27. Which parts of D24 to D26 need compiler support

- **Decision:** `set!` is prelude code with compiler rules that borrow a non-`Copy` field as a place and let `at` name an element inside `set!`; `new` gains a base value for `assoc` on a struct, `match` gains struct patterns, and `fn` types gain inferred closure kinds. `D28` adds one kernel form, `assoc-at`, because a collection picks its Rust by its type, and `assoc` and `assoc-in` stay prelude macros over it.
- **Rejected:** One kernel form per feature: `set!`, `take` and a closure-kind type.
- **Why:** Each feature fits inside a form the compiler already knows, which keeps the compiler small and the prelude large.
- **Rule:** Superseded by [D28](#d28-the-library-additions-from-dogfooding-round-2) for `assoc-at`; [Kernel forms](../docs/language.md#kernel-forms) and [Ownership](../docs/language.md#ownership) for the rest

## D28. The library additions from dogfooding round 2

- **Decision:** Add the input and output, map, sequence and string functions from dogfooding round 2. `slurp`, `slurp-stdin`, `read-line`, `command-line-args` without the program name, `exit` and `io-error` cover input and output, `sorted-map` and `hash-map` share one API that the compiler picks by type, and sequences and strings gain their Clojure-style functions; a `main` that returns an error prints it with `display` and exits 1.
- **Rejected:** Separate names per map kind; a hidden runtime map trait that emits unreadable generic Rust; Rust's `Debug` output for `main`'s error; the program name in `command-line-args`.
- **Why:** The API reads like Clojure, hides Rust's details, and copies nothing the source does not write.
- **Rule:** [Standard types](../docs/language.md#standard-types)

## D29. Helpers used at one type

- **Decision:** A function whose every call in the program uses the same concrete types emits those types, as `is_alive(grid: &[Vec<bool>], x: usize, y: usize) -> bool`. Calls at two or more types keep it generic, and a written type signature stays exact.
- **Rejected:** Always generic, with type pins in the source; specializing only functions a module does not export.
- **Why:** This matches what a person writes by hand and keeps programs free of type pins, and a second type changes only the emitted Rust, never behavior.
- **Rule:** [Types](../docs/language.md#types)

## D30. Error conversion in `try`

- **Decision:** A program enum's variant with one payload of an error type declares the conversion, as `(enum app-error (io io-error) (parse (parse-error i64)))` lets `try` turn either error into `app-error`. It emits `impl From<E>` for the enum once per conversion a `try` uses, keeps `expression?`, and the first `try` fixes an unwritten error type, so a converting function writes a type signature naming the enum.
- **Rejected:** An explicit `(impl from ...)`; `try-with` naming the conversion at each site; inferring the error type from the body's tail or from a visible enum.
- **Why:** No new syntax or kernel form is needed, the Rust keeps plain `?`, and the error type stays a choice the program writes once, as in Rust.
- **Rule:** [9.1 Propagating errors with `try`](../docs/language.md#91-propagating-errors-with-try)

## D31. `display` for a program's types

- **Decision:** `(display point (p) "(" (field p x) ", " (field p y) ")")` lists printing parts as `println` does and emits `impl std::fmt::Display` with `write!`, allocating nothing. A body that is one `match` or `if` writes in each arm, only a struct or enum of the program implements `display` (L0445), `prn` keeps the data form, and no `display` is inferred.
- **Rejected:** A body that returns a string, one allocation per print; an explicit `impl` with the formatter, `write` and `try`; names `printable` or `show`.
- **Why:** It reads like `println`, hides Rust's formatter, and emits the Rust a person writes.
- **Rule:** [10. User-defined traits](../docs/language.md#10-user-defined-traits)

## D32. Template hygiene and result variants

- **Decision:** A template marks each name its Rust text binds as `{!name}`, renamed only on a clash. `ok`, `err` and `some` are prelude macros over the variant constructors, so `(ok (try e))` emits `e` when the `try` converts nothing, and they can no longer be passed as function values; an enum clause `(rust "Result" bare)` emits bare `Ok`, `Err`, `Some` and `None`.
- **Rejected:** Discovering closure binders in template text; reserving template names in the program; keeping `ok` and `err` as functions with `Ok(e?)`.
- **Why:** Correct programs, hand-written Rust, and no compiler knowledge of prelude template text.
- **Rule:** [Hygiene](../docs/language.md#hygiene) and [9. Enums and `match`](../docs/language.md#9-enums-and-match)

## D33. Milestone 11, written lifetimes

- **Decision:** A function whose result holds a borrow gets one shared inferred lifetime, as `fn longer<'a>(a: &'a str, b: &'a str) -> &'a str`. A written lifetime is a `'a` symbol in `where` and head binders and in `(ref 'a t)`, and is exact; a struct or enum that stores a borrow gets one inferred lifetime parameter, and a written lifetime used only once emits elided, as `b: &str`.
- **Rejected:** Provenance inference; no inference beyond elision; a `lifetimes` binder form; written lifetimes at every use of a borrowing type; a full lifetime checker; lifetimes inferred per impl.
- **Why:** Lifetimes stay hidden in the common case, the emitted Rust stays sound and readable, and the compiler stays small; provenance inference can replace the shared lifetime later without changing programs.
- **Rule:** [11. Written lifetimes](../docs/language.md#11-written-lifetimes)

## D34. Generic implementations and the rest of traits

- **Decision:** An unknown name in an impl's type is a type variable, so `(impl sized (vec a) ...)` emits `impl<A> Sized for Vec<A>` with inferred bounds. A trait declares an associated type with `(type item)` and an impl states it with `(type item a)`, a trait method may carry a default body, `(trait (shape (display)) ...)` declares a supertrait, and `(box shape)` is `Box<dyn Shape>` built by `box-new` for object-safe traits only.
- **Rejected:** Declared impl variables; generic traits; Rust's `IntoIterator`; per-impl helper delegation for defaults; repeated supertrait bounds; enums in place of dynamic dispatch.
- **Why:** The source stays short and Clojure-like, Rust's own model sits under it, and each step unlocks the next example.
- **Rule:** [10. User-defined traits](../docs/language.md#10-user-defined-traits); `iter` is superseded by [D35](#d35-one-iter-for-vectors-and-maps-2026-10-26)

## D35. One iter for vectors and maps (2026-10-26)

- **Decision:** There is no iterable trait. `(iter m)` on a map emits `m.iter()` and yields `(pair (ref k) (ref v))`; `iter` is a `map-at` operation that picks the item type from the collection, and a generic "any iterable" waits for a program that needs it.
- **Rejected:** An `iterable` trait implemented for `(ref (vec a))`, as Rust's `IntoIterator`, which needs impl lifetimes and whose receiver never matched; a generic associated type `(type (item 'a))`, which is new syntax and rarely used in Rust; both were prototyped and failed.
- **Why:** Both alternatives put lifetimes into user trait definitions, so hiding Rust's low-level details favors no iterable trait.
- **Rule:** [Standard types](../docs/language.md#standard-types)

## D36. Order of the round 3 fixes (2026-10-26)

- **Decision:** Fix box and trait bugs first, then ownership and lifetime bugs with research first, then library additions, then diagnostics and emitted Rust, then run one `nix flake check`.
- **Rejected:** Bugs only; library first.
- **Why:** Fix the newest feature's bugs before adding surface.
- **Rule:** not stated in docs/language.md

## D37. Library names from dogfooding round 3 (2026-10-26)

- **Decision:** Use Clojure names: `(subs s start end)`, `pi`, `(eprintln ...)` and `(remove-at! v index)`, and make `parse-error` implement `display` and `debug`.
- **Rejected:** Rust names: `slice-text` and `remove!`.
- **Why:** The language reads like Clojure.
- **Rule:** [Standard types](../docs/language.md#standard-types)
