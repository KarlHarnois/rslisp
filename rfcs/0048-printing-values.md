# RFC 0048: Printing values in the language's syntax

**Status:** Accepted
**Date:** 2026-11-03

## Problem

`prn` and `pr-str` print a value with Rust's `Debug`, so a user sees Rust where the language promised its own syntax:

```text
> (->point 4 3)
Point { x: 4, y: 3 }
> (shape/circle 1.0)
Circle(1.0)
> (some (vec 1 2))
Some([1, 2])
> (pair 1 (char "a"))
(1, 'a')
```

None of these lines is a form the reader reads: `{`, `}`, `[` and `]` are reader errors, and `'a'` is a symbol. The REPL of RFC 0047 prints every result through `prn`, and its `--json` mode sends `pr-str` text to the editor as the `value` field, so the REPL needs this change first.

The language cannot fix it today. `prn` and `pr-str` are `rust` templates over `{:?}`, and the compiler derives `Debug` for every struct and enum whose fields allow it. Rust's `Debug` for `Vec`, `Option`, `String` and the tuples prints Rust, and no program can change a standard library impl.

Today's `Debug` form also leaves two gaps:

- `unit`, `(sorted-map k v)` and `(hash-map k v)` do not implement `debug`, so `(prn (do))` and `(prn (sorted-map "a" 1))` are `L0404`.
- A struct with a function field derives `Debug`, which prints the function's address: `(prn (->op twice))` printed `Op { run: 0x55cb17f7c100 }` and then `Op { run: 0x56533fd74100 }` on two runs of one program.

The numbers below come from `main` at `12fda36b` on an 18-core Linux machine, with rustc 1.95.0 from `nix develop`, the compiler built by `nix develop -c cargo build --release`, and a prototype of option A in a scratch worktree outside the repository. The probes of today's written `derive`, `rust-type` and `L0445` behavior ran on a release build of `main` at `18701feb`, whose code is that of `12fda36b`.

### The printed form

This section answers what each value prints as. Every option below prints these forms, and differs only in where the code that prints them lives.

**A printed value is a form that the reader reads.** It uses only atoms and lists, so it needs no new reader character. Where the language can build the value from source, the printed form is the source that builds an equal value, so a user can paste a result back into a program. The prototype printed each row, except the enum, error and user-impl rows, which come from option B's emitted Rust written by hand and compiled with rustc:

| Value | Prints as | Reads back equal |
| --- | --- | --- |
| `42`, `(the u8 7)`, `-3` | `42`, `7`, `-3` | Yes. An integer literal takes the type its place expects. |
| `1.0`, `(the f32 0.1)`, `1e100` | `1.0`, `0.1`, `1e100` | Yes. Rust's shortest round-trip text, which always holds a `.` or an `e`. |
| infinity, negative infinity, NaN | `(/ 1.0 0.0)`, `(/ -1.0 0.0)`, `(/ 0.0 0.0)` | Infinity, yes. NaN equals no value, itself included. |
| `true` | `true` | Yes |
| `(char "a")`, `(char "\"")` | `(char "a")`, `(char "\"")` | Yes |
| a `(ref str)`, a `string` | `"hello \"ana\"\n"` | As a `(ref str)`. Only the escapes the reader knows: `\n \r \t \\ \0 \" \u{...}`. A place that needs a `string` needs `(str ...)` around the text, as decision 1 accepts. |
| `unit` | `(do)` | Yes |
| `(some 3)`, `none` | `(some 3)`, `none` | Yes, where the place gives `none` its type |
| `(ok 1)`, `(err (str "bad"))` | `(ok 1)`, `(err "bad")` | Yes, where the place takes a `(ref str)` error |
| `(pair 1 "a")` | `(pair 1 "a")` | Yes |
| `(vec 1 2 3)`, an empty vector | `(vec 1 2 3)`, `(vec)` | Yes |
| a `(slice a)` view | `(vec 1 2 3)` | No: the text builds the owned vector the view borrows from. |
| `(ref x)` | what `x` prints | No: the text builds the value, not the borrow. |
| `(box 5)` | `(box 5)` | Yes |
| `(sorted-map "b" 2 "a" 1)` | `(sorted-map "a" 1 "b" 2)` | Yes, in key order |
| `(hash-map "zed" 1 "amy" 2)` | `(hash-map "amy" 2 "zed" 1)` | Yes, sorted by each key's printed text, as decision 3 chose. |
| `(->point 4 3)` | `(->point :x 4 :y 3)` | Yes, as decision 2 chose, where no field is a `string` |
| `(shape/circle 1.0)`, `shape/empty` | `(shape/circle 1.0)`, `shape/empty` | Yes |
| a variant with a struct payload, `(labeled (struct (text string) (size f64)))` | `(shape/labeled "box" 2.0)` | Only where no field is a `string`. A construction takes such fields by position, and a `string` field rejects `"box"`, `L0401`, as decision 1 accepts. |
| a recursive enum | `(tree/node tree/leaf 1 tree/leaf)` | Yes. The `Box` that the compiler adds prints as nothing, because the source never writes it. |
| `io-error`, a `parse-error` | `(io-error "No such file or directory (os error 2)")` | No: it reads but does not evaluate, as decision 4 accepts. |

Nested values nest their forms: `(->holder :item (some (->point :x 1 :y 2)) :label "box")`. A printed value never holds a newline, because a string's newline prints as `\n`, so RFC 0047's `--json` mode still finds a value as the rest of one line.

A number prints without a type suffix. The language has no suffix syntax, and `(the u8 7)` in every element of a `(vec u8)` would bury the data. The type of a printed value comes from its place, as a literal's type does in source.

**Values with no printed form.** A closure, an iterator and a function value have no source that rebuilds them, so they do not implement the printing trait. `(prn (iter v))` is `L0404` at compile time, as it is today, with a help line that names the reason. The REPL prints RFC 0047's note with the value's type. A struct or enum whose field or payload holds a function type does not implement the printing trait either, so no printed text ever holds an address that changes between runs.

## Options

The options differ in where the printing code lives. Each one prints `(prn (->point 4 3))` as `(->point :x 4 :y 3)`.

### A: a prelude trait, with impls that the definers write

The prelude declares a trait `show` backed by a runtime trait, and states its impls for the built-in types in `prelude/runtime/show.rs`. `prn` and `pr-str` print through it:

```lisp
(rust-trait show "crate::rt::Show"
  (method write-form (fn ((ref self) (mut formatter)) fmt-result))
  (help "a closure, an iterator or a function has no printed form")
  bool char str string unit io-error i8 i16 i32 i64 i128 isize u8 u16 u32 u64 u128 usize f32 f64
  (parse-error bool) (parse-error char) (parse-error string) (parse-error i8) (parse-error i16) (parse-error i32) (parse-error i64) (parse-error i128) (parse-error isize) (parse-error u8) (parse-error u16) (parse-error u32) (parse-error u64) (parse-error u128) (parse-error usize) (parse-error f32) (parse-error f64)
  (where ((a show)) (ref a))
  (where ((a show)) (vec a))
  (where ((a show)) (slice a))
  (where ((a show)) (option a))
  (where ((a show) (b show)) (result a b))
  (where ((a show)) (box a))
  (where ((a show) (b show)) (pair a b))
  (where ((k show) (v show)) (sorted-map k v))
  (where ((k show) (v show)) (hash-map k v)))

(def prn (rust "println!(\"{{}}\", crate::rt::Shown({0}))" (where ((a show)) (fn ((ref a)) unit))))
(def pr-str (rust "crate::rt::Shown({0}).to_string()" (where ((a show)) (fn ((ref a)) string))))
```

```rust
pub trait Show {
    fn write_form(&self, out: &mut Formatter<'_>) -> Result;
}

impl<T: Show> Show for Option<T> {
    fn write_form(&self, out: &mut Formatter<'_>) -> Result {
        match self {
            Some(value) => Record::open(out, "some").part(value).close(),
            None => out.write_str("none"),
        }
    }
}
```

The `io-error` and `parse-error` rows give decision 4's form, so `(prn (slurp "missing.txt"))` prints `(err (io-error "No such file or directory (os error 2)"))`, and `(prn (the (result i64 parse-int-error) (parse "x")))` prints `(err (parse-int-error "invalid digit found in string"))`.

`defstruct` writes an `impl` beside the struct, through prelude templates over the runtime's `Record`, which writes `(->point` for the head:

```lisp
(defstruct point :x i64 :y i64)
(defn main () (prn (->point 4 3)))
```

expands to:

```lisp
(struct point (x i64) (y i64))
(def make-point (fn (x y) (new point (x x) (y y))))
(impl show point
  (write-form (value out)
    (record-close (record-field (record-field (record-construct out "point") ":x" (ref (field value x)))
                                ":y" (ref (field value y))))))
(def main (fn () (prn (make-point 4 3))))
```

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Point {
    x: i64,
    y: i64,
}

fn make_point(x: i64, y: i64) -> Point {
    Point { x, y }
}

fn main() {
    crate::rt::restore_default_sigpipe();
    println!("{}", crate::rt::Shown(&make_point(4, 3)))
}

impl crate::rt::Show for Point {
    fn write_form(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        crate::rt::Record::construct(out, "point")
            .field(":x", &self.x)
            .field(":y", &self.y)
            .close()
    }
}
```

The compiler changes only by one runtime part. The prototype breaks in two ways that no prelude change can fix:

- **An impl is unconditional.** A macro sees forms, not types, so `defstruct` cannot tell whether every field type implements `show`. The impl body then fails at the definition, and a struct that stores a function, a kernel `enum` or a `rust-type` without `show` stops compiling. On the corpus, `evaluator` no longer checks:

  ```text
  error[L0404]: `expr` does not implement `show`
   --> tests/fixtures/corpus/evaluator/parser.rslisp:6:1
    |
  6 | (defstruct parsed :node expr :next usize)
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ this needs `show`
  ```

- **`enum` and `struct` are kernel forms.** No prelude macro runs for them, so a kernel `struct` and every program `enum` print nothing until their author writes an impl by hand.

### B: a prelude trait, with impls that the compiler infers

The trait, its built-in impls, `prn` and `pr-str` are those of option A. A struct or enum implements `show` when every field or payload type does, as it implements `debug` today, and a function type blocks `show`, as it blocks `partial-eq`.

Rust has no derive for the trait, so the prelude states the impl's Rust as a template, in a new `structural` clause of `show`'s `rust-trait` declaration in `prelude/text.rslisp`:

```lisp
(rust-trait show "crate::rt::Show"
  (method write-form (fn ((ref self) (mut formatter)) fmt-result))
  (structural
    :struct "crate::rt::Record::open({1}, \"->{name}\"){parts}.close()"
    :field ".field(\":{field}\", {value})"
    :variant "crate::rt::Record::open({1}, \"{name}\"){parts}.close()"
    :part ".part({value})"
    :bare "{1}.write_str(\"{name}\")")
  (help "a closure, an iterator or a function has no printed form")
  bool char str string unit io-error ...)
```

The emitter fills the template as it fills a `rust` template today:

- `{0}` and `{1}` are the method's parameters, the receiver and the formatter, in the order `(method write-form ...)` gives them.
- `{name}` is the struct's name, or a variant's qualified name such as `tree/node`, escaped for a Rust string literal.
- `{field}` is a field's name, and `{value}` is a borrow of that field or payload. For a payload in the `Box` that a recursive enum requires, `{value}` borrows the boxed value, as `&**left`, because the source never writes that `Box`.
- `{parts}` is the struct's fields, each filled into `:field`, or a variant's payloads, each filled into `:part`, in declared order. A variant with a struct payload fills `:part`, because a construction takes its fields by position.
- A struct fills `:struct`, a variant with a payload fills `:variant`, and a variant without one fills `:bare`.

Each key takes only these placeholders, besides `{{` and `}}`:

- `:struct` takes `{0}`, `{1}`, `{name}` and `{parts}`.
- `:variant` takes `{0}`, `{1}`, `{name}` and `{parts}`.
- `:bare` takes `{0}`, `{1}` and `{name}`.
- `:field` takes `{field}` and `{value}`.
- `:part` takes `{value}`.

Everything else in the impl comes from facts the compiler already reads from the prelude's bindings. The trait path is the `rust-trait`'s string, the method's name and signature come from its `method` clause, as for an `impl` that a program writes today, and each type parameter of the struct or enum is bounded by that path. The `match self` of an enum's impl is the emitter's own `match` emission. So the compiler names no `Show`, `write_form`, `Record` or method of `Record`.

Inference finds `show` by its clause, not by its name. Today the compiler infers the Rust derives in `StructDerives::INFERRED`, a fixed list of Rust derive names, and matches each to a prelude `rust-trait` by its path. Option B infers, beside that list, every `rust-trait` whose declaration holds a `structural` clause, so the compiler finds `show` through the prelude's bindings and never by name. Decision 5 takes `"Debug"` off the list, and the list names no prelude trait.

**One rule for the clause, on every `rust-trait`.** The prelude's `rust-trait` and a program's follow the same rule:

- A `structural` clause needs exactly one `method` clause, whose first parameter is `(ref self)`, because `{0}` and `{1}` name that method's parameters and the impl has one body. A clause on a `rust-trait` with no `method`, with several, or with a first parameter other than `(ref self)` is a new expansion error, `L0248`, worded in `ExpandProblems` beside `L0246`: "a `structural` clause needs one method that takes `(ref self)`", with its label on the clause and the help "declare one `(method name (fn ((ref self) ...) result))` in this `rust-trait`".
- A `structural` clause on a `rust-trait` whose path a fixed derive writes, `Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`, `PartialOrd`, `Ord` or `Hash`, compared by the last segment of the path as `derive` compares it today, is `L0248`, because every struct would get both the derive and the template impl. Its message reads "`cl` binds `std::clone::Clone`, which `derive` already writes", with the help "keep the clause off this `rust-trait`, or bind a trait that `derive` does not write". So is a clause on a `rust-trait` whose path another `rust-trait` binds, the case that `L0217` warns about today, because the two would infer two impls of one Rust trait. Its message reads "`num2` binds `crate::rt::Number`, which `number` already binds", with the help "keep the clause off this `rust-trait`, or give it a Rust path that no other `rust-trait` binds". A program's clause at `crate::rt::Show` is this case. No inferred impl can then collide with a derive or with another trait's impl.
- The clause body is keyword and string pairs. A struct needs `:struct` and `:field`, and an enum needs `:variant`, `:part` and `:bare`. A clause holds the keys of every kind of type, so any struct or enum can take it. A missing key, a key other than these five, a repeated key, a key without a string after it, and a placeholder that its key cannot fill, such as `{field}` in `:part` or `{2}` when the method has two parameters, are `L0248` too, with the label on that key or placeholder and the message naming it, as in "`:bare` is missing from this `structural` clause".
- A `rust-trait` with the clause is inferred for every struct and enum, as `show` is, whichever module declares it. A program's own `rust-trait` takes the clause as the prelude's does. Its path names a trait outside the crate, as `docs/language.md` requires: "`rust-trait` binds only traits that Rust defines". A program writes no Rust item, so a `crate::` path that the runtime does not define is rustc's `E0432`, "unresolved import", today and stays so. With the prelude's `formatter` and `fmt-result` written as the program's own `rust-type`s:

  ```lisp
  (rust-type octal-out "std::fmt::Formatter<'_>")
  (rust-type octal-result "std::fmt::Result")
  (rust-trait octal "std::fmt::Octal"
    (method fmt (fn ((ref self) (mut octal-out)) octal-result))
    (structural :struct "write!({1}, \"{name}\")" :field "" :variant "write!({1}, \"{name}\")" :part "" :bare "write!({1}, \"{name}\")"))
  ```

  Every struct and enum of the program whose fields allow it then implements `octal`. The same declaration without the clause, and with an `(impl octal mode (fmt (m out) ...))`, compiles today and emits `impl std::fmt::Octal for Mode`.

`docs/language.md` says that "the compiler fixes this list, because a marker on `rust-trait` would need syntax that reads as a type application". The `structural` clause is not that marker. A marker such as `inferred` or `(inferred)` among a `rust-trait`'s implementors would read as one more implementor type. `structural` holds a keyword and a string in each pair, and no type holds a string, so the clause is told from an implementor as `(help "text")` is today. It is a clause of the `rust-trait` kernel form because only the compiler knows a struct's fields and whether each field's type implements the trait, which is why option A fails.

```lisp
(enum (tree a) leaf (node (tree a) a (tree a)))
(defn main () (prn (tree/node tree/leaf 1 tree/leaf)))
```

```rust
#[derive(Clone, PartialEq, Eq, Hash)]
enum Tree<A> {
    Leaf,
    Node(Box<Tree<A>>, A, Box<Tree<A>>),
}

impl<A: crate::rt::Show> crate::rt::Show for Tree<A> {
    fn write_form(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tree::Leaf => out.write_str("tree/leaf"),
            Tree::Node(left, value, right) => crate::rt::Record::open(out, "tree/node")
                .part(&**left)
                .part(value)
                .part(&**right)
                .close(),
        }
    }
}

fn main() {
    crate::rt::restore_default_sigpipe();
    println!(
        "{}",
        crate::rt::Shown(&Tree::Node(Box::new(Tree::Leaf), 1, Box::new(Tree::Leaf)))
    );
}
```

`(prn (->point 4 3))` emits this impl, and the `main` of option A, and its struct derives `Clone, Copy, PartialEq, Eq, Hash` without `Debug`, as decision 5 chose:

```rust
impl crate::rt::Show for Point {
    fn write_form(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        crate::rt::Record::open(out, "->point")
            .field(":x", &self.x)
            .field(":y", &self.y)
            .close()
    }
}
```

The emitter writes an impl for a struct or enum when a reached use of `show` needs it, directly, as `(prn p)` needs `point`'s, or through an implementor's bound, as printing a `(vec point)` needs `point`'s through the row `(where ((a show)) (vec a))`, and printing a `(tree point)` needs both `tree`'s and `point`'s. A program that never prints a type emits no impl for it.

**The struct head.** `defstruct` names the constructor `->point`, and the `:struct` template writes `->` before the name, so the `->` lives in two prelude files, `prelude/struct.rslisp` and `prelude/text.rslisp`. Keeping it in two files is part of what decision 2 accepted: a template is a literal string, so it cannot take the prefix from a definition that `defstruct` states. A fixture that prints a struct and reads the text back catches a change to one file alone. A kernel `struct` has no `->point`, so its value prints a form that does not evaluate, which decision 2 also accepted.

**A written `derive`.** `derive` accepts every trait whose `rust-trait` holds a `structural` clause, beside its fixed Rust derive names, and a listed one emits its impl from the template, never a `#[derive]` name. `(derive ticket copy ord hash show)` therefore emits `#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]` and the `show` impl. Today `(derive p display)` is `L0422`, with the help "a `derive` lists any of: clone, copy, debug, partial-eq, eq, partial-ord, ord, hash". That help then lists the traits with a `structural` clause too, read from the prelude's and the program's bindings, so it ends with `show`. A written `derive` lists a type's traits exactly, `show` included, as it does today for every trait. A type with a written `derive` prints only when its `derive` lists `show`, and a written `derive` that lists `show` over a field whose type does not implement `show` is `L0423`, as for any listed trait. A written `debug` needs `debug` on every field type, and after decision 5 a program struct or enum implements `debug` only when its own written `derive` lists it.

**A written `impl` of a `structural` trait replaces the inferred impl of that trait for the type's head.** This is a rule of its own, and it holds for every trait whose `rust-trait` holds a `structural` clause, `show` and a program's alike: a written `impl` turns off inference for that trait only, and the type keeps every other inferred trait. The rule keys on the head, so a written impl for any instance of a generic program type turns inference off for the whole type: `(show (tree i64) (t) ...)` leaves `tree` with no inferred `show`, and `(prn (the (tree bool) tree/leaf))` is `L0404`. An inferred `impl<A: Show> Show for Tree<A>` beside the written one would be rustc's `E0119`. A type whose written `derive` lists a `structural` trait and that also has a written `impl` of it is `L0459`, the error below, with a note on the `derive`.

**A program writes its own form** with a prelude macro over `impl`, as `display` is a macro over `impl` today. The body is one construction form, a list whose head is a symbol, and each argument prints with `show`:

```lisp
(defstruct money :cents i64)
(defn dollars (amount) (->money :cents (* amount 100)))
(show money (m) (dollars (/ (:cents m) 100)))
(defn main () (prn (->money :cents 1200)))
```

```rust
impl crate::rt::Show for Money {
    fn write_form(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        crate::rt::Record::open(out, "dollars")
            .part(&(self.cents / 100))
            .close()
    }
}
```

It prints `(dollars 12)`. A body that is a bare symbol writes the symbol's name, so `(show origin (o) origin)` prints `origin`. Any other body, such as a literal or a list whose head is not a symbol, is a prelude `fail`, reported as `L0301` as every prelude `fail` is today, with the message "a `show` body is one construction form or a symbol" and the help "write the call that builds the value, as in `(->money :cents 1200)`". The prelude states that wording, as `defstruct` states its own. The `Record` calls in this impl come from the macro's `rust` templates in `prelude/text.rslisp`, not from the compiler.

**Types from `rust-type` and enums with a `rust` clause.** `L0445` stays as it is: a program writes an impl of a `rust-trait`, `show` included, only for its own structs and enums. A type that a `rust-type` binds implements `show` only through a row of `show`'s `rust-trait` in the prelude, and a field or payload of such a type without one blocks `show`, as it blocks `debug` today: with `(rust-type (rc a) "std::rc::Rc<{0}>")`, `(defstruct holder :item (rc i64))` does not implement `show`, and `(prn (->holder (rc 1)))` is `L0404`, as the `debug` form is today. An enum with a `(rust "Path")` clause gets no inferred impl, because its traits come from `rust-trait` rows only and it emits no Rust definition. The runtime's impls for `option`, `result` and `pair` stay the only impls of those three.

**A second impl of a trait for one program type is a new type error, `L0459`, worded in `TypeProblems`.** It covers two written impls of one trait whose implementors unify, such as two `(display p ...)` forms, or `(display (tree i64) ...)` beside `(display (tree a) ...)`, and a `derive` that lists a `structural` trait beside a written impl of it for any instance of the type. The checker reports it before emission, so rustc's `E0119` never reaches the user: today two `(display p ...)` forms give `error[E0119]: conflicting implementations of trait std::fmt::Display for type p`, and `(display (tree i64) ...)` beside `(display (tree a) ...)` gives `error[E0119]: conflicting implementations of trait std::fmt::Display for type tree<i64>`, both reported by rustc. `L0459` reads "`p` already implements `display`", with its label on the second impl, a note on the first impl or the `derive`, and the help "remove one of the two". A written impl of a method-less `crate::` trait reaches rustc or a later check today, as `(impl signed point)` gives `E0277`, and this RFC leaves it as it is.

### C: write Rust `Debug` impls in the language's syntax

The emitter replaces `#[derive(Debug)]` with a written `impl std::fmt::Debug` that prints the struct's form, and `prn` keeps `{:?}`. A program's own types then print as forms, but every standard library type keeps Rust's `Debug`, which no crate can replace:

```lisp
(prn (some (->point 4 3)))
```

```text
Some((->point :x 4 :y 3))
```

A wrapper type in the runtime that prints a standard type's form is the trait of options A and B under another name.

### D: choose the printing code by type at each call

`prn` becomes a kernel rule, as `map-at` is, and the compiler emits nested printing code for the static type of each argument. A generic function bounded by printing has no static type to expand, so it needs a trait anyway, and the rule restates in Rust every form that the prelude states for `option`, `result` and `pair`.

### Prior art

- **Clojure** prints readably with `pr` and `prn`, which dispatch on the `print-method` multimethod, so a program extends printing per type. A record prints as `#user.Point{:x 4, :y 3}`, which the reader reads through `#`. EDN is the readable subset. A function prints as `#object[...]`, which reads as an error.
- **Common Lisp** prints through the generic function `print-object`. A structure prints as `#S(POINT :X 4 :Y 3)`, which reads back. With `*print-readably*` true, printing an object that has no readable form signals `print-not-readable`, where `print-unreadable-object` writes `#<...>` otherwise.
- **Racket** has `write`, `display` and `print`. `print` writes a value as an expression that produces it, so a transparent struct prints as `(point 4 3)`, the closest precedent for this design. The `print-as-expression` parameter switches it to `write` style, and `prop:custom-write` lets a struct print its own form.
- **Scheme** (R7RS) `write` writes an external representation that `read` reads, with datum labels such as `#0=` for shared structure, and `display` writes text for people.
- **Carp**, a Lisp compiled to C, generates `str` and `prn` for every `deftype` through its `str` interface, and prints a value as `(Point 4 3)`.
- **Gerbil** inherits Gambit's `write`, which prints a struct as `#<point #3>`. Its `:std/misc/repr` library prints a representation that rebuilds the value, through a `:pr` method that a type implements.
- **Chicken** prints a record as `#<point>` until the program sets a printer with `set-record-printer!`.
- **Rust**'s derived `Debug` bounds each type parameter by `Debug`, and fails on a field that lacks it. This compiler already decides per struct whether to derive it, and option B keeps that rule for `show`.

Every printer above that reads back marks unreadable values with `#`. This language has no `#`, so a value with no source form does not print, as Common Lisp does under `*print-readably*`, and the error comes at compile time.

### Measurements

Front-end instructions and allocated bytes come from `cargo xtask bench measure` on `main` and on option A's prototype, which adds the `show` trait and its runtime part, prints `prn` and `pr-str` through it, and makes `defstruct` write the impl. No corpus entry calls `prn`, so these numbers are the cost of the impls alone. Emitted size is `rslisp emit`'s output in bytes. Rustc time is the median CPU time, user and system, of 7 runs of `rustc --edition 2024` in debug on the emitted file. Other work held the machine's load average between 16 and 23 during the rustc runs, so a rustc difference under about 15% is noise.

| Entry | Structs | Instructions, `main` | Option A | Allocated bytes, `main` | Option A | Emitted bytes, `main` | Option A | rustc, `main` | Option A |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `generic_types` | 61 | 583.7M | 668.2M (+14.5%) | 75.1M | 84.9M (+13.0%) | 74,778 | 101,887 | 1.74 s | 2.12 s |
| `many_modules` | 120 | 1,412.9M | 1,675.1M (+18.6%) | 146.5M | 176.5M (+20.5%) | 147,138 | 206,500 | 0.82 s | 1.06 s |
| `matrix_geometry` | 5 | 203.7M | 215.0M (+5.5%) | 28.2M | 29.7M (+5.1%) | 20,400 | 29,084 | | |
| `warehouse` | 3 | 180.8M | 191.6M (+6.0%) | 26.5M | 28.1M (+6.1%) | 26,985 | 35,307 | 0.82 s | 0.88 s |
| `call_chain` | 0 | 1,198.2M | 1,200.0M (+0.2%) | 146.1M | 146.3M (+0.1%) | 46,367 | 46,367 | | |
| `evaluator` | 2 | 305.7M | fails to check | 41.4M | fails to check | | | | |

Option A emits an impl for every reached struct, and a program that prints none of them pays for each one in every stage. Option B emits an impl only for a type whose `show` a reached function uses, so on this corpus, where nothing prints, its emitted Rust is `main`'s without `Debug` in each derive. Its front-end cost is one more trait in the struct traits inference, which `debug` pays today. Option B was not built, so that cost is not measured.

Dropping the inferred `Debug` from every derive shrinks the emitted Rust by under 1% (`many_modules`: 147,138 to 146,298 bytes) and gives no rustc difference above the noise: 1.94 s for `generic_types`, 0.71 s for `many_modules` and 0.82 s for `warehouse`.

`(prn (->point 4 3))` emits the impl of option A and this `main` under both A and B:

```rust
fn main() {
    crate::rt::restore_default_sigpipe();
    println!("{}", crate::rt::Shown(&make_point(4, 3)))
}
```

Today it emits `println!("{:?}", make_point(4, 3))`. The prototype's runtime `show` part is 235 lines, which a program carries only when its code names `crate::rt::Show`.

## Recommendation

Option B, with the printed forms of the table above. The owner accepted it, with the choices under Decision.

- It prints every value that the source can build as the source that builds it, so a REPL result pastes back into a program.
- The printing code for built-in types, the impl template for program types, and every character of the printed syntax live in the prelude: `prelude/text.rslisp` and `prelude/runtime/show.rs`. The compiler infers every trait whose `rust-trait` holds a `structural` clause and fills that clause's template with a type's names and parts, so it names no prelude trait, method or runtime type.
- A struct or enum prints exactly when its fields allow it, the rule `debug` follows today. These programs that compile today stop compiling:
  - A `prn` or `pr-str` of a value whose type holds a function, `L0404`. Today it prints an address.
  - A `prn` or `pr-str` of a type whose written `derive` does not list `show`, `L0404`. `tests/fixtures/run/written_derive.rslisp` writes `(derive ticket copy ord hash debug)` and then `(prn early)`, which prints `Ticket { id: 1, seat: 4 }` today and becomes `L0404` at `(prn early)` and `(prn same)`. The build adds `show` to that `derive`, and the fixture then prints `(->ticket :id 1 :seat 4)` twice.
  - A written `derive` that lists `debug` over a field whose type is a program struct or enum without its own written `debug`, `L0423`, as `(defstruct inner :x i64) (derive inner clone) (defstruct outer :i inner) (derive outer debug)` is `L0423` today.
  - A call with a program type at a `debug` bound that the program writes, such as `(type log (where ((a debug)) (fn ((ref a)) unit)))`, `L0404`, unless the type's written `derive` lists `debug`.
  - A function whose written signature bounds a parameter by `debug` and whose body prints it, `L0404` in the body, because `prn` now needs `show` and a written signature is exact: `(type log (where ((a debug)) (fn ((ref a)) unit))) (defn log (x) (prn x))` prints `Holder { item: 1 }` today.
  - A program trait with `debug` as a supertrait, implemented for a program type without a written `debug`, `L0446` at the impl: with `(trait (loggable (debug)) ...)`, `(impl loggable p ...)` is `L0446` today when `p`'s written `derive` leaves `debug` out, and every program type leaves it out after decision 5.
  - A program `rust-trait` whose path is `std::fmt::Debug` and that lists a program type, `L0421`, as `(derive point clone) (rust-trait loud "std::fmt::Debug" point)` is `L0421` today. The same declaration also gets the warning `L0217`, "`loud` binds a Rust path that `debug` already binds", today and after this RFC.
  - A `(pr-str x)` part of `println`, `print` or `display` whose value's type does not implement `show`, `L0404`, as for `pr-str` itself.
  - Two written impls of one trait whose implementors unify, `L0459` where rustc reported `E0119`.
  - A call of `unwrap-ok`, `unwrap-err` or `expect-ok` whose other side does not implement `show`, such as an error type that holds a function, `L0404`.
- It costs no allocation in the emitted Rust: the impl writes into the formatter as Rust's derived `Debug` does. The one allocation inside the runtime is the sort of a `hash-map`'s printed keys.
- A program that never prints a type emits no impl for it, so the emitted Rust of a program without `prn` is unchanged.

Option A fails on the corpus and leaves enums out. Option C keeps Rust's text for every standard type. Option D needs the trait anyway.

The rest of the language follows:

- **`debug` and Rust's `Debug` derive.** `prn`, `pr-str` and the `(pr-str x)` part of `println`, `print` and `display` use `show`. Nothing in the language then calls `Debug`, so the struct traits rule stops inferring `debug` and the emitted types stop deriving it, as decision 5 chose. `debug` stays a `rust-trait` for interop, and a written `(derive point debug)` still adds it.
- **`unwrap-ok`, `unwrap-err` and `expect-ok`.** Rust's `Result::unwrap`, `unwrap_err` and `expect` require `Debug` of the other side, to print it in the panic message. Once program types stop deriving `Debug`, a call on a `result` whose other side is a program type would fail in rustc. The build therefore moves these three to a runtime part that panics with the `show` form, as in `called unwrap-ok on (err (parse-failure :line 3))`, and gives them a `show` bound on the other side, which a written signature then states. `option`'s `unwrap` and `expect` need no bound.
- **`display`, `println` and `str`.** Unchanged: they print user-facing text with Rust's `Display`. `(str (str "ana"))` is `ana` and `(pr-str (str "ana"))` is `"ana"`.
- **Generic functions.** A function bounded by `(a show)` prints any implementor. A body that calls `prn` on a parameter infers the bound, as a call to `<` infers `ord`.
- **RFC 0047.** This RFC replaces `debug` with `show` in RFC 0047's rule 3, including its sentence "A result prints when its type implements `debug`, with the Rust that `prn` emits", in its rule 5, including its statement of `pr-str`'s bound `(where ((a debug)) (fn ((ref a)) string))`, and in the `note` and `value` rows of its `--json` table. The REPL prints a result when its type implements `show`, its note for a type that does not names `show`, and the `value` field holds the printed form, which never spans two lines. RFC 0047's status line records that those parts are superseded by this RFC.
- **Snapshots.** Every snapshot that `prn` writes changes once. Before 1.0 this needs no migration aid.

## Decision

The project owner accepted option B on 2026-11-03, with these choices:

1. **Owned strings.** A `string` prints as `"ana"`, as a `(ref str)` does, the way Clojure and Rust print both. The text is shorter, and reading it back into a place that needs a `string` needs `(str ...)` around it.
2. **The struct head.** A struct prints as `(->point :x 4 :y 3)`, the call by name of RFC 0038 and the closest form to Clojure's `#user.Point{:x 4, :y 3}`. The `->` lives in `prelude/struct.rslisp` and in `show`'s `structural` template in `prelude/text.rslisp`, and a kernel `struct` prints a form that does not evaluate.
3. **`hash-map` order.** A `hash-map` prints its entries sorted by each key's printed text, so one program prints the same text on every run. Printing allocates one string per key.
4. **Errors from Rust.** `io-error` and each `parse-error` type print as their type applied to their `display` text, such as `(io-error "No such file or directory (os error 2)")`. The text reads but does not evaluate.
5. **The inferred `Debug` derive.** The compiler stops inferring `debug`, so the emitted types no longer derive `Debug` once `prn` and `pr-str` stop using it. A written `derive` still lists it.
6. **The name.** The trait is `show`. The name is chosen for this RFC only: the language has no naming rule for traits yet. The build names the method and the runtime's types.

## Rejected options and why

- **An owned `string` printed as `(str "ana")`:** it reads back as a `string`, but it doubles the text of every string field and element, and Clojure and Rust print both kinds of string alike.
- **`(->point 4 3)` for a struct:** it names no field, and its text breaks when a field moves.
- **`(new point (:x 4) (:y 3))` for a struct:** it evaluates for every struct and needs no prelude fact, but no user writes it.
- **A `hash-map` in iteration order:** `HashMap`'s random seed changes the text between two runs of one program, so a snapshot or a REPL result would change at random.
- **No printed form for `io-error` and `parse-error`:** `(prn (slurp path))` would be a compile error.
- **Keeping the inferred `Debug` derive:** nothing in the language calls it once `prn` uses `show`.
- **A: impls that the definers write:** a macro cannot tell whether a field's type prints, so a struct with a function field or a kernel `enum` field stops compiling, and `enum` and `struct` are kernel forms that no macro wraps.
- **C: Rust `Debug` impls:** every standard library type keeps Rust's text, and no crate can change it.
- **D: printing code chosen by type at each call:** a generic function still needs a trait, and the compiler would restate the prelude's forms for `option`, `result` and `pair`.
- **A printed value marked with a new reader character, such as `#`:** the reader treats only `(`, `)`, `"`, `;` and whitespace specially.
- **A type suffix on every number:** the language has no suffix syntax, and `(the u8 7)` on every element buries the data.

## Rule

The build that lands this design states its rule in [docs/language.md](../docs/language.md), in the sections on standard types, struct traits, enums, Rust interop and user-defined traits. It replaces the sentence "`prn` and `pr-str` print a value's data form" with the printed forms of each kind of value, replaces `debug` with `show` in the struct traits rule, replaces "The compiler fixes this list" with the rule that the compiler infers every `rust-trait` with a `structural` clause, states the `structural` clause in the `rust-trait` row of the kernel forms table, and records why only the compiler can infer a `structural` trait: it knows each field's type and whether that type implements the trait. It rewords the sentence at `docs/language.md:257`, "The compiler fixes this list, because a marker on `rust-trait` would need syntax that reads as a type application", to say that the compiler fixes the Rust derive names and infers every trait whose `rust-trait` holds a `structural` clause. It states that `derive` accepts such a trait and emits its impl from the template, and that `L0422`'s help lists those traits from the prelude's and the program's bindings beside the fixed derivable ones. It adds `L0248` and `L0459` to `docs/diagnostics.md`'s codes with a fixture each, and leaves `L0445` as it is.

This RFC is 0048 while PR 281, which also claims 0048, is open. Whichever of the two merges second takes the next free number.
