# The rslisp language

`rslisp` is a placeholder name. Source files use the `.rslisp` extension.

The language is a Lisp that compiles to Rust. Its design follows four principles:

1. **Few rules, no exceptions.** A feature that needs an exception gets redesigned or left out.
2. **Small compiler, big prelude.** Anything the language can express lives in the prelude, written in the language itself. The compiler only reads, checks, reports and emits.
3. **Rust's performance and Rust's model.** Ownership, borrowing, traits and generics work as in Rust. The compiler never adds a hidden clone or allocation, except the `Box` a recursive enum payload requires.
4. **Few special characters.** The reader treats only `(`, `)`, `"`, `;` and whitespace specially.

## Syntax

The reader knows four rules:

1. A file is a sequence of forms.
2. A form is an atom or a list: `( form* )`.
3. An atom is a string, a number or a symbol.
4. `;` starts a comment that runs to the end of the line.

### Atoms

| Atom | Examples | Rule |
|---|---|---|
| String | `"hello"`, `"tab\there"` | Double quotes, with Rust's escapes: `\n \r \t \\ \0 \" \u{...}`. |
| Number | `42`, `-7`, `1_000`, `2.5`, `1e9` | A token that starts with a digit, or with `+` or `-` followed by a digit. Such a token must be a valid number. |
| Symbol | `point`, `make-point`, `admin?`, `+`, `...` | Every other token. |

There is no quote shorthand, no `#` syntax and no `[]` or `{}`. The reader rejects `[`, `]`, `{` and `}` so that code from other Lisps fails with a clear message.

## Meaning

1. Each kernel form says what each of its positions holds: an expression, a binder, a type, a field label or a module path. The position table below lists them.
2. A symbol in an expression or type position refers to the binding that its name, namespace and scopes resolve to (see Hygiene).
3. Types and values are separate namespaces, and the position picks one. `str` can name both a type and a macro.
4. In an expression position, a list whose head resolves to a kernel form or a macro is that form. Any other list is a call. In a type position, a list is a type application.
5. A binder is `name` or `(name annotation)`, everywhere a name is bound. For a value the annotation is a type. For a type variable it is the traits the variable implements.

The kernel's names are bindings exported by the built-in `kernel` module, and the prelude re-exports them. A user can shadow `if` with `(let if 3)`, and macros that expand to `if` still get the kernel form.

`()` in an expression is an error. `(do)` is the unit value.

## Kernel forms

These forms are the compiler's complete vocabulary. Every other form is a macro or a function from the prelude.

| Form | Shape | Rust | Positions |
|---|---|---|---|
| `fn` | `(fn (param...) body...)` | `fn` item, closure, or block | params: binders. body: expressions. |
| `def` | `(def name value)` | `fn`, `const`, `static`, or nothing | name: binder. value: expression. |
| `let` | `(let name value)` | `let` | name: binder. value: expression. |
| `do` | `(do form...)` | block | forms: expressions. At module level: definitions. |
| `if` | `(if test then else)` | `if`/`else` | all: expressions. |
| `type` | `(type name type)` | signature | name: reference to a `def`, or to a macro whose value position expands to a function the macro introduced in this module. type: type. |
| `struct` | `(struct name (field type)...)` or `(struct (name param...) (field type)...)` | `struct` | name: type binder. params: type binders. fields: field labels with types. |
| `enum` | `(enum name (rust "Path")? variant...)` or `(enum (name param...) (rust "Path")? variant...)` | `enum`, or the named Rust type | name: type binder. params: type binders. `rust`: an optional Rust path. variants: a name, `(name type...)`, or `(name (struct (field type)...))`. |
| `match` | `(match value (pattern body)...)` | `match` | value: expression. patterns: a wildcard, a literal, a bare name, `(variant pattern...)` or `(type-name pattern...)`. arms: one body expression. |
| `derive` | `(derive type trait...)` | `#[derive(...)]` | type: reference to a `struct` or `enum` of this module. traits: references to traits. |
| `new` | `(new name (field value)...)` or `(new _ base (field value)...)` | struct literal or struct update | name: type, or `_` with a base expression. base: expression. fields: field labels with expressions. A label is a field name or a keyword, as in `(new car (:year 2025))`. |
| `assoc-at` | `(assoc-at value (key...) item)` | block | value: expression. keys: expressions. item: expression. |
| `map-at` | `(map-at operation value argument...)` | method call or block | operation: `lookup`, `keys`, `vals`, `entries`, `iter` or `update`. value: expression. arguments: expressions. |
| `set-at` | `(set-at value (key...) item)` | assignment | value: expression. keys: expressions. item: expression. |
| `field` | `(field value name)` | `value.name` | value: expression. name: field label. |
| `ref` | `(ref value)` | `&value` | value: expression. |
| `mut` | `(mut place)` | `&mut place` | place: expression. |
| `deref` | `(deref value)` | `*value` | value: expression. |
| `quote` | `(quote form)` | none | form: data. |
| `macro` | `(def name (macro (form) body...))` | none | form: binder. body: expressions. |
| `rust` | `(rust "template" type)` | the inlined template | type: type. |
| `recur` | `(recur argument...)` | `loop` | arguments: expressions. |
| `try` | `(try expression)` | `expression?` | expression: expression. |
| `rust-trait` | `(rust-trait name "Path" (method name type)? (help "text")? type...)` | trait bound, or a Rust `impl` for a program type when it declares a method | name: type binder. methods: a name and a function type whose first parameter may be `self`, `(ref self)` or `(mut self)`. help: the help line a missing implementor shows. types: types, `(with type type)`, or `(where (param...) type)`. |
| `trait` | `(trait name (method type)...)` | `trait` | name: type binder. methods: a name and a function type whose first parameter may be `self`, `(ref self)` or `(mut self)`. |
| `impl` | `(impl trait type (method (param...) body...)...)` | `impl` | trait and type: references to a trait and a struct or enum. methods: a name, parameter binders and a body. |
| `rust-type` | `(rust-type name "template" (type "template")...)` or `(rust-type (name param...) "template" (type "template")...)` | the filled template | name: type binder. params: type binders. clauses: a type and a template. |
| `use` | `(use module name...)` | path | module: module path. names: binders. |
| `export` | `(export name...)` | `pub` | names: references in every namespace. |

In type positions, the kernel provides `fn`, `fn-mut`, `fn-once`, `ref`, `mut`, `where`, `with`, `_`, and the primitive types `bool char str string unit i8 i16 i32 i64 i128 isize u8 u16 u32 u64 u128 usize f32 f64`.

In expression positions, the kernel provides `ref`, `mut` and `deref`, and the constants `true` and `false`. The constants are kernel constants, not prelude definitions. Macro code and program code then see the same booleans, so `if`, `and` and `or` work in both.

`ref`, `mut` and `deref` are kernel forms because a prelude function cannot mark its result as written. The compiler must see the written form to know that the value came from a written borrow or dereference.

`assoc-at` is a kernel form because only the compiler knows a collection's inferred type. A prelude macro cannot choose between a vector and a map, so the `assoc-at` collection rule picks the emitted Rust by type. The rule knows `(vec a)`, `(sorted-map k v)` and `(hash-map k v)`.

`map-at` and `set-at` are kernel forms for the same reason. Only the compiler knows a map's key and value types, so one rule picks the emitted Rust for `lookup`, `keys`, `vals`, `entries`, `update` and `set!` from the inferred map type. `map-at` also carries `iter`: it accepts a vector, a slice or a map and emits `.iter()`, yielding `(ref a)` for a vector or a slice and `(pair (ref key) (ref value))` for a map. The prelude cannot express `iter` as a function, because one signature cannot accept both a slice and a map without an iterable trait, and only the compiler knows the collection's inferred type and the item type to yield. The prelude macros expand to these forms, and the collection rules for `assoc-at`, `map-at` and `set-at` share the map type test.

### Module bodies

A module body is expanded in two passes:

1. Each top-level form is expanded in order until its head is a definition form: `def`, `type`, `struct`, `enum`, `derive`, `rust-trait`, `rust-type`, `trait`, `impl`, `use`, `export` or `do`. The names it defines are registered. A `def` whose value is a `macro` is evaluated right away.
2. `def` values, types and `export` lists are expanded.

Values and types are therefore order-independent, like Rust items. A macro must appear above its first top-level use.

### Bodies

A body is the sequence of forms in `fn`, `macro` and `do`. A `let` binds its name for the rest of the enclosing body. The value of a body is the value of its last form, or unit if the body is empty or ends with a `let`.

A function that calls itself in tail position compiles to a Rust loop. The call reassigns the parameters that change, then starts the next iteration. A call to itself anywhere else stays recursive. A `fn` that the program calls where it is written emits as a block that binds each parameter with `let`.

## Types

Type syntax mirrors value syntax:

| Type | Rust |
|---|---|
| `i64` | `i64` |
| `(fn (i64 i64) i64)` | `fn(i64, i64) -> i64` |
| `(fn-mut (i64) i64)` | `impl FnMut(i64) -> i64` |
| `(fn-once (i64) i64)` | `impl FnOnce(i64) -> i64` |
| `(ref str)` | `&str` |
| `(ref 'a str)` | `&'a str` |
| `(mut point)` | `&mut Point` |
| `(mut 'a point)` | `&'a mut Point` |
| `(pair i64 string)` | `(i64, String)` |
| `(where ((n number)) (fn (n) n))` | `fn<N: Number>(N) -> N` |
| `_` | inferred |

A function type's kind is `fn`, `fn-mut` or `fn-once`. A written `fn-mut` or `fn-once` is exact, a plain `fn` leaves the kind to inference, and an inferred function never needs a written kind. Type variables are bound by a `where`, by a `struct`'s head or by a `rust-type`'s head. A type definition names its parameters on its name, as `(struct (pair a b) (first a) (second b))` and `(rust-type (boxed item) "Box<{0}>")` do. A `where` binds them over what its form holds: a signature's type, a `rust-trait` implementor or a function type. An unknown type name is an unbound-name error that suggests the closest name.

A lifetime is a symbol that starts with `'`. It is declared beside a type parameter: in a `where` for a function, or in a struct or enum head, as `(struct (snippet 'a) (text (ref 'a str)))` does. A reference names one as an optional first part: `(ref 'a t)` and `(mut 'a t)`. A written lifetime is exact. An elided reference leaves its lifetime to Rust's elision.

A bound names the trait and, optionally, its second type: `(a (partial-eq b))` is Rust's `A: PartialEq<B>`. The second type must be a type the program knows or a binder of the same `where`, as in `(where ((a (partial-eq b)) b) (fn (a b) bool))`. A plain bound keeps Rust's default: `(a partial-eq)` is `A: PartialEq<A>`. A trait takes a second type exactly when one of its `rust-trait` entries uses `with`.

A struct or enum whose fields or payloads store a borrow gets one inferred lifetime parameter before its type parameters. A bare `str` or `(slice a)` written as a field or payload is such a borrow, exactly as `(ref str)` or `(ref (slice a))` is, because Rust cannot store an unsized view by value. The source keeps writing the bare type name, and a lifetime written in the head is exact and replaces the inferred one. A function that returns such a value whose borrowed field or payload holds a borrow of a local gets `L0510`, with a help that names the field and the owned type to declare, `string` for `str` and `(vec a)` for `(slice a)`. Each type parameter of a struct must appear in a field. Each parameter of a Rust type must appear in its template. A type parameter takes no traits, because Rust would demand a type's traits at every use. A function that needs a trait names it in its own `where`. A type takes exactly as many arguments as it has parameters.

```lisp
(struct (pair a b) (first a) (second b))
(type swap (where ((a clone) (b clone)) (fn ((ref (pair a b))) (pair b a))))
```

```lisp
(type add (fn (i64 i64) i64))
(defn add (x y) (+ x y))

(type identity (where (a) (fn (a) a)))
(defn identity (x) x)

(type scale (fn (f64 _) _))
```

Inference is Hindley-Milner with trait bounds. When a definition is generalized, a two-type bound whose second type is another still-unknown variable unifies the two, so the scheme has one parameter with a plain bound. Top-level functions generalize to Rust generics. Mutually recursive functions generalize together, so each one declares the type parameters its type mentions. Local `let` bindings stay monomorphic, because Rust closures cannot be generic.

After inference, a function whose every use outside its own recursive group gives each type parameter the same concrete type emits those types in place of the parameters, as `is_alive(grid: &[Vec<bool>], x: usize, y: usize) -> bool` does. A function used at two or more types stays generic. A written `type` signature stays exactly as written.

A type parameter satisfies a trait when its `where` names the trait, or when every type its traits allow implements the trait. `(n float)` therefore satisfies `number` and `display`. The emitted Rust names any such bound that the runtime traits' supertraits do not already give.

### Literals

- An integer literal can become any number type, so `(->point 1 2)` emits `make_point(1.0, 2.0)`.
- A float literal can become `f32` or `f64`.
- Every literal is checked against the range of its type.
- A call pins a literal parameter the way it pins any inferred parameter, so a helper called once with a `usize` keeps `usize` in its parameter and its literals.
- A use pins a literal in a function's result the same way, so `(defn port () 8080)` passed only where a `u16` is expected emits `fn port() -> u16`.
- A literal whose type is still open after pinning defaults to `i64` or `f64`, and the emitted literal carries a suffix (`3_i64`).
- Without a signature, a literal never makes a function generic: `(defn increment (x) (+ x 1))` has type `(fn (i64) i64)`.
- A signature's type parameter opts in. A literal can take the parameter's type if it fits every type the parameter's traits allow, which are the implementors those traits share:

  ```lisp
  (type increment (where ((n number)) (fn (n) n)))
  (defn increment (x) (+ x 1))        ; emits x + N::from_literal(1)

  (type halve (where ((n float)) (fn (n) n)))
  (defn halve (x) (* x 0.5))          ; emits x * N::from_float_literal(0.5)
  ```

  `(+ x 300)` with `(n number)` is an error, because `n` can be `u8`. `(* x 0.5)` with `(n number)` is an error, because `n` can be an integer type. A literal whose parameter has no trait is an error, because the parameter can be any type. The conversion is exact for every allowed type, so Rust folds it into a constant.
- A string literal has type `(ref str)`.

### Standard types

The prelude binds these Rust types, the traits that abstract them, and the functions that use them:

| Type | Rust | Functions |
|---|---|---|
| `(vec a)` | `Vec<A>` | `vec push! len iter values collect get pop! remove-at! sort! sort-by-key! windows at dedup! first last empty? reverse reverse! sort-descending!` |
| `(slice a)` | `[A]` | `iter windows at first last empty? reverse` |
| `(box a)` | `Box<A>` | `box-new` |
| `(pair a b)` | `(A, B)` | `pair pair/pair let-pair` |
| `(option a)` | `Option<A>` | `some none option/some option/none some? none? unwrap-or unwrap expect map-some then-some` |
| `(result a b)` | `Result<A, B>` | `ok err result/ok result/err ok-or ok? err? unwrap-ok unwrap-err expect-ok map-ok map-err then-ok` |
| `(iterator a)` | `impl Iterator<Item = A>` | `range map filter fold for-each enumerate zip sum count max min max-by-key any all find take take-while` |
| `char` | `char` | `alphanumeric? whitespace? digit?` |
| `str`, `string` | `&str`, `String` | `to-string display str char print println eprintln chars lines split trim subs as-str parse parse-error from-str lower-case upper-case split-when append! append-char! empty?` |
| `(sorted-map key value)` | `std::collections::BTreeMap<K, V>` | `sorted-map lookup keys vals entries iter update into-sorted-map assoc set! len empty?` |
| `(hash-map key value)` | `std::collections::HashMap<K, V>` | `hash-map lookup keys vals entries iter update into-hash-map assoc set! len empty?` |

`print`, `println` and `eprintln` take any number of parts. Each string literal part folds into one format string, and each other part becomes a `{}` placeholder. `eprintln` writes to standard error. `str` folds its literal parts the same way. A `print` whose last part ends in a newline emits `println!` without that newline. `prn` and `pr-str` print a value's data form. `(display type (value) parts...)` implements `display` for a struct or enum the program defines, and makes the type printable with `print`, `println`, `str` and `to-string`. `subs` takes a `str`, a start and an end, and returns the `str` between them, so `(subs line 2 5)` emits `&line[2..5]`. `pi` is Rust's `std::f64::consts::PI`, an `f64`. `remove-at!` removes the element at an index and returns it, so `(remove-at! items 0)` emits `items.remove(0)`.

`len` takes any `sequence`, a trait backed by `Sequence` in the runtime and implemented for `String`, `str`, `(vec a)`, `(slice a)`, `(sorted-map k v)` and `(hash-map k v)`. `empty?` tests the same trait. `sort!`, `sort-by-key!`, `max`, `min` and `max-by-key` take the `ord` trait, which lists the same implementors as `partial-ord`, except `f32` and `f64`. A two-argument `sort!` takes a comparator of type `(fn ((ref a) (ref a)) i64)`, which sorts as in Clojure: a negative result puts the left value first. `parse` takes a `from-str` target and returns `(result target (parse-error target))`, and emits `text.parse::<target>()`. `parse-error` is the associated type `Err` of `from-str`, so `(parse-error i64)` is `parse-int-error` and emits `std::num::ParseIntError`, and `(parse-error target)` on a type parameter emits `<Target as std::str::FromStr>::Err`. The prelude exports each value of `parse-error`: `parse-bool-error`, `parse-char-error`, `infallible`, `parse-int-error` and `parse-float-error`. Every `parse-error` a target names implements `display` and `debug`, so `(to-string reason)` and `(prn reason)` print it.

An iterator is lazy, as in Rust, so `(fold (range 1 1000001) 0 (fn (total n) (+ total n)))` repeats work without recursion. `(vec 1 2 3)` emits `vec![1, 2, 3]`, and `(vec)` emits `Vec::new()`.

`sorted-map` builds a `BTreeMap` from key and value pairs, and `(sorted-map)` is empty. Its keys implement `ord`, and its entries iterate in key order. `hash-map` builds a `HashMap` the same way; its keys implement `hash` and `eq`, and its iteration order is not fixed. `into-sorted-map` and `into-hash-map` collect a vector of pairs. `lookup` returns `(option (ref value))`, and a `str` may query a `string` key. `keys`, `vals` and `entries` return iterators of references. `iter` on a map returns the same iterator as `entries`. `update` removes the value or uses the default, applies its function, inserts the result and returns the map, so no value is copied. `assoc` inserts a key and returns the map. `set!` on `(at m k)` borrows the key and emits `*m.get_mut(&k).unwrap() = v`; it changes an existing key and panics on a missing one. `len` and `empty?` take a map through `sequence`. The `map-at`, `assoc-at` and `set-at` collection rules pick all of this from the inferred map type, so the prelude has one API for every map. A key that lacks its map's key traits is the existing missing-trait error.

Program input and output use Rust's standard library through the prelude. `slurp` reads a file, `slurp-stdin` reads all of standard input, and both return `(result string io-error)`. `read-line` returns `(result (option string) io-error)`, returns `none` at end of input, and drops a trailing newline and carriage return. `io-error` is Rust's `std::io::Error`, which implements `display` and `debug`. `command-line-args` returns the arguments after the program name. `exit` stops the process with an `i32` code.

A `main` whose inferred type is a `result` emits an inner function for its body and a `fn main()` that prints its error with `display` to standard error and exits 1. The success path exits 0. The error type must implement `display`; a program's own enum implements it with `display`.

- `iter` yields `(ref a)` for a vector or a slice, and `(pair (ref key) (ref value))` for a map. `values` yields copies, so its elements must be `copy`.
- `filter` passes each element to its test as a reference, as Rust does.
- `range` takes an `integer`, a prelude trait of the integer types.
- An `iterator` cannot be a struct field.
- A function generic over `integer` can call `range`: the prelude's `step` trait states Rust's requirement `Range<N>: Iterator<Item = N>`, and the function's emitted signature carries it as a `where` predicate.

## Structs

Programs write structs with the prelude's `defstruct`, which takes flat fields. `(defstruct point :x f64 :y f64)` declares each field as a keyword followed by one type form, so `:tags (vec str)` is one field. It expands to the kernel `struct` form and defines one constructor, `->point`.

The constructor `->point` is a macro, and it reads a call in this order:

1. If the arguments are `:field value` pairs that name distinct fields and every field without a default, the call is by name. `(->point :y 2.0 :x 1.0)` expands to `(new point (:y 2.0) (:x 1.0))`, in the order written.
2. Otherwise, if the arguments are `:key value` pairs, the call is positional only when it has one argument per field and every keyword at an even position lands on a field whose written type is a function type, `fn`, `fn-mut` or `fn-once`. With `(defstruct sorter :key (fn ((ref user)) (ref str)) :label str)`, `(->sorter :name "by name")` is positional. A type parameter does not count as a function type.
3. Otherwise, `:key value` pairs are a call by name. The call expands to `new` with the defaults of the omitted fields, and `new` reports an unknown key in the user's spelling with the closest field, as `(->car :mak "Nissan")` suggests `:make`, a repeated key, or the missing fields that have no default.
4. Any other call with exactly one argument per field is positional. `(->point 1.0 2.0)` calls a positional function that `defstruct` defines under a hygienic name, which no source code can refer to. It emits as `make_point(1.0, 2.0)`. Any other call is an error that names the field count.

In value position, `->point` expands to the positional function, so `(let build ->point)` emits `let build = make_point;`. A program may define its own `make-point`, because no source name refers to the positional function.

A `defstruct` may end with one `(defaults :field value ...)` clause, recognized by its `defaults` head, in a place where a field keyword would go. Each key names a declared field at most once. `(defstruct user :name str :email str :admin? bool (defaults :admin? false))` lets a call by name omit `:admin?`, and `(->user :name "ana" :email "ana@example.org")` expands to `(new user (:name "ana") (:email "ana@example.org") (admin? false))`, which emits `User { name: "ana", email: "ana@example.org", is_admin: false }`. The default is inlined at each construction that omits it, so it takes its field's type there, and a default such as `(vec)` builds a new value at each construction. A default cannot refer to other fields. A positional call and `->user` in value position still take every field.

### Field keywords

A keyword reads a field. A list whose head is a keyword and that has one argument expands to the kernel `field` form: `(:x origin)` is `(field origin x)`, and it borrows, moves and reports an unknown field exactly as that form does. A keyword in value position expands to a closure that reads the field, so `(map (iter points) :x)` emits `points.iter().map(|value: &Point| value.x)`. Each such closure is typed where it is used.

A keyword call with no value or with more than one value is an error. `let`, `def`, `fn` parameters and patterns reject a keyword as a name. The rule runs on the expanded program, so a macro receives a keyword unchanged, as `defstruct` does.

A field label in `new` may be written as a keyword, so a macro passes the user's spelling through, as `assoc` and `->car` do. An unknown field then reports and suggests the field as a keyword, as `(assoc leaf :yer 2025)` suggests `:year`.

A field read names its struct only through the value's type. When two structs share a field name, a function that reads that field from an unannotated parameter needs a signature, as `(type volume (fn ((ref cube)) i64))` gives.

The expander carries keyword reads because a prelude macro per field would clash between two structs that share a field name.

## Ownership

Ownership is part of the type, as in Rust. `(ref t)` is `&T` and `(mut t)` is `&mut T`. Type inference ignores ownership except where it is written, and a separate pass decides how each parameter is passed.

- **Modes.** A parameter without a written type gets the cheapest mode its uses need: copy for a Copy type of 16 bytes or less, then ref, then mut, then own. A type whose size the compiler cannot compute, such as a type parameter, counts as 16 bytes or less. A use through a call follows the called function's mode, and the modes are recomputed until nothing changes.
- **Copy types.** A Copy type implements the prelude's `copy` trait, whose Rust path is `Copy`. A type parameter is Copy when its `where` names `copy`. The parameter is also Copy when one of its traits allows only Copy types. When a body copies a generic parameter that no written signature declares, the compiler infers the parameter's `copy` bound. A written signature is exact: if it declares the parameter without `copy`, the second use is an error that suggests `(a copy)`.
- **Written types.** A type written without `ref` or `mut`, in a `type` signature or a binder annotation, means owned, as in Rust.
- **Adjustment.** Wherever a value meets an expected type, the compiler adds one borrow, removes extra borrows, copies a Copy value of 16 bytes or less out of a borrow, or shrinks a shared borrow's referent from a vector to its slice or from `string` to `str`. A vector or map where the expected type is `(iterator a)` is iterated: the compiler writes `.iter()`, yielding `(ref a)` for a vector and `(pair (ref key) (ref value))` for a map. A vector yields copies instead, by `.iter().copied()`, when the expected item is a Copy type that the vector's elements are, or a type parameter whose traits allow only Copy values, such as `number`. Elements from number literals are any number type their literals allow. `(sum scores)` emits `sum(scores.iter().copied())`, and with a `(iterator (ref str))` parameter, `(count-long names)` on a `(vec (ref str))` emits `count_long(names.iter().copied())`. It never clones. The compiler writes each adjustment in the emitted Rust, except the slice and `str` ones: Rust's deref coercion already makes `&Vec<T>` meet `&[T]` and `&String` meet `&str`, so no syntax changes. The slice adjustment lets a read-only vector parameter take a slice, and the `str` adjustment lets a read-only `string` parameter take `&str`. `(add x 2)` with `x : (ref i64)` passes where Rust's generic `add(x, 2)` fails, because the compiler writes the `*`. The Call sites and Dereferences bullets give the details. A written `ref`, `mut` or `deref` is not adjusted: it meets a parameter with a written type as written, and a parameter with no written type as the mode its body infers.
- **Call sites.** An argument is borrowed automatically when the parameter is a reference. A method-style template such as `"{0}.clone()"` borrows its receiver the way Rust's method calls do. A receiver or a `{&0}` argument is written without `&`, even when it reads a field, because Rust borrows it. A `{&0}` argument stays usable after the call: `(if (> a b) a b)` moves neither `a` nor `b` until it returns one of them. A receiver that names a static is written without `*`, because a method call dereferences it.
- **Dereferences.** A borrowed Copy argument of 16 bytes or less is dereferenced when the parameter's type is a type variable whose traits the reference lacks. `(> x 2)` with `x : (ref i64)` emits `*x > 2`. A `fn` passed as an argument takes its parameter types from the parameter's function type, so its body sees them. An argument with more reference layers than its reference parameter passes with the extra layers removed, as Rust's deref coercion does, even when the parameter's referent is still unknown. If an argument's referent type is still unknown at a call, the compiler decides its borrow after it infers the definition. A borrowed non-Copy argument passes as is when the parameter's type is a type variable whose traits the referent has and the reference lacks: `(size-of (as-str word))` emits `size_of(word.as_str())`.
- **Shared parameters.** When one type variable is the whole type of several parameters, as in `(= a b)`, the compiler decides their borrows together after it infers every argument, so the order of the arguments never changes the result. Two type variables linked by a two-type bound form one group. The compiler first tries the arguments exactly as written: if the bound holds with each variable bound to its argument's type, every argument passes with no borrow. Otherwise it applies the shared rule. A Copy value of 16 bytes or less is copied out of every borrow. Arguments with the same number of references pass as they are. Otherwise, each argument gets one reference, or none where Rust borrows the argument by itself: `(= (:name p) name)` emits `p.name == name`. `partial-eq`, `partial-ord` and `display` hold for a reference to any type that has them, as in Rust: `(< (:name ana) (:name bo))` emits `ana.name < bo.name`.
- **Fields.** Reading a Copy field of 16 bytes or less copies it. Reading any other field borrows it: `(field person name)` has type `(ref string)`.
- **Unsized values.** Rust never owns a `str`. A `str` parameter is always borrowed, and a function whose result is a `str` returns `(ref str)`.
- **No hidden copies.** Copying heap data is always written `(clone x)`, which needs the `clone` trait. The one inferred allocation is the `Box` a recursive enum payload requires. A struct implements it when its fields do, as the Struct traits bullet says. `(ref x)` and `(mut x)` force a borrow.
- **Struct traits.** A struct implements `clone`, `debug`, `partial-eq`, `eq` and `hash` when all its field types do, and the emitted struct derives them. It implements `copy` too when all its field types are Copy and the program never changes it in place. A function-typed field allows `clone`, `copy` and `debug`, and blocks comparison and hashing, because Rust compares function pointers unreliably. The compiler fixes this list, because a marker on `rust-trait` would need syntax that reads as a type application. `prn` and `pr-str` print a value's data form. A written `(derive point clone debug)` lists a struct's traits exactly and turns inference off for it. A listed trait brings the traits Rust requires of it: `copy` brings `clone`, `eq` and `partial-ord` bring `partial-eq`, and `ord` brings `eq` and `partial-ord`. `derive` is a kernel form, because the prelude cannot add attributes to an emitted struct. The program changes a struct in place when a `mut` borrow takes a value of the struct or a place that reads one of its fields, or when a written type is `(mut s)`.
- **Moves.** The compiler reports a use after move and points at the move. It suggests `(clone x)` only when the clone keeps the meaning: the value must not change in place after the move, must not be a `mut` borrow, and must not be passed to a recursive call of the enclosing function. Otherwise the help names the real fix. A move in either branch of an `if` counts.
- **Function values.** A named function passed where a function type is expected is wrapped in a closure when its parameters need an adjustment. The compiler decides the adjustments after ownership inference, so a parameter whose inferred mode adds a borrow also wraps the function value. An expected parameter that is a reference and a function parameter that is not is the same rule before modes. The closure passes each argument through the usual adjustments. Both function types first take the types the call gives their type parameters, so an unannotated `(defn apply-one (f a) (f a))` called as `(apply-one increment 41)` expects `(fn (i64) i64)`. If no adjustment makes a parameter match, the compiler reports a mismatched function type and names the parameter.
- **Reborrows.** A `mut` reference passed to a `mut` parameter is reborrowed and stays usable. Passed to a `ref` parameter, it is reborrowed as a shared reference, as Rust coerces `&mut T` to `&T`. Passed to any other parameter, it moves.
- **Mut borrows.** A `mut` argument must be a place: a local or a field. A field is borrowed in place, even when the field is not Copy. A copy, such as a call's result, is an error, because changing the copy changes nothing.
- **Assignment.** `(set! place value)` changes a local or a field in place. Inside the place, `(at items index)` names an element, so `(set! (at (at grid y) x) alive)` emits `grid[y][x] = alive`. A value-position `(at items index)` returns a borrow of the element, so a non-Copy row reads without `get` and `unwrap`. A Copy element meeting a value parameter is copied out, so `(at (at grid 1) 0)` emits `grid[1][0]` there and `&grid[1][0]` where it stays borrowed. A map element assigns through `get_mut`, so `(set! (at m k) v)` borrows `k`, emits `*m.get_mut(&k).unwrap() = v` and panics on a missing key. A target that is not a place is the same error as a `mut` argument. `set!` is prelude code: a local or field target assigns through a `mut` parameter, and an `at` target becomes a vector or map assignment.
- **Value updates.** A keyword key updates a field, and every other key updates a collection. `(assoc value :field value...)` returns a changed struct with Rust's struct update syntax, as `(assoc cell :alive true)` emits `Cell { alive: true, ..cell }`. `(assoc coll key value...)` and `(assoc-in coll (key...) value)` consume the collection and return it changed, as `(assoc grid 1 alive)` emits `{ let mut grid = grid; grid[1] = alive; grid }`, and each further key and value pair updates the result of the one before. A collection update whose last key has no value is an error, and when its keys look like `(field value)` groups, the help shows the keyword form. A struct passed as a collection gets the type error for a base that is not a collection, with help that points to keywords. The base moves, so a later use is a use-after-move error whose help suggests `(clone base)`. `assoc` and `assoc-in` are prelude macros. The `assoc-at` collection rule picks the emitted Rust from the base's inferred type; it knows `(vec a)`, `(sorted-map k v)` and `(hash-map k v)`, and a map update emits `{ let mut m = m; m.insert(k, v); m }`.
- **Call order.** A call that mutably borrows a variable evaluates its other arguments first. A borrow has no side effect, so the observable order stays the same.
- **Borrowed results.** A function may return a borrow. When its parameters contribute more than one lifetime, the compiler adds one shared lifetime, `'a`, to the reference parameters whose borrows can reach the result and to the result, so the result may borrow from any of them. Every other reference parameter stays elided. With one, Rust's elision decides. A function-typed parameter whose call can return the result takes the shared lifetime in every reference position of its type, because Rust's elision does not reach through a function type: `(fn ((fn ((ref str) u16) car)) car)` emits `fn build_with<'a>(constructor: impl FnOnce(&'a str, u16) -> Car<'a>) -> Car<'a>`. It stays elided when a call of it in the body passes a borrow of a local, because that borrow cannot live as long as `'a`. A trait or impl method without a receiver follows the same rule. A returned borrow of a local value is an error, `L0510`.
- **Closures.** A closure's body decides its kind. A closure that moves a capture, or passes it to a parameter that owns it, is `FnOnce`. One that changes a capture, or passes it to a `mut` parameter, is `FnMut`. Every other closure is `Fn`. A function parameter that the body calls at most once and uses no other way is `FnOnce`. One called more than once, or called while it changes a capture, is `FnMut`, with a `mut` parameter binding. One only passed on takes the kind its callee needs. A signature writes `(fn-mut ...)` or `(fn-once ...)` to pin a kind, and that written kind is exact; a plain `(fn ...)` leaves the kind to inference. The prelude's `rust` templates write the kind their Rust body requires. `Fn` meets `FnMut` and `FnOnce`, and `FnMut` meets `FnOnce`, as in Rust. A closure stronger than the parameter accepts is an error, `L0509`; its label names the capture and its help says to clone it or to borrow it. A function-typed parameter emits `impl Fn`, `impl FnMut` or `impl FnOnce`, and so does a returned closure behind its return type. A closure returned from a function is a `move` closure. A closure that only borrows or copies its captures is Copy and can be passed more than once. A local closure that changes a captured local emits as `let mut`, because Rust takes a `FnMut` closure mutably.
- **rustc.** Any lifetime or aliasing error that slips past these checks comes from rustc, mapped back to the source and the user's names.

## Macros

A macro is a function from form to form that runs at compile time:

```lisp
(def defn
  (macro (form)
    (list (quote def) (nth form 1)
          (prepend (quote fn) (drop 2 form)))))
```

Macro bodies run in the compiler's interpreter. They use the kernel forms, the built-in `code` module, and functions that use only those. A `rust` definition belongs to the program, so macro code cannot run `+`, `<` or any other Rust template. To compute in the program, a macro returns the code, as in `(template (+ (unquote x) 1))`.

The compiler checks every macro body and the functions it reaches, even a macro nothing calls. A mistake is reported where the macro is defined, even in a branch that never runs.

A macro also answers for its name in value position. When a program uses a macro's name as a value, as in `(let build ->car)`, the expander calls the macro with the bare symbol as its form. If the macro returns a form, the expander uses that form in place of the name. If the macro fails, the program gets the error `is a macro, not a value`. A macro that handles value position tests its form with `symbol?`. A macro that takes its form apart with `first`, `rest` or `nth` fails on a symbol, so it keeps the error. The prelude cannot provide this, because a macro runs only at the head of a list, and only the expander sees a macro's name standing alone.

The prelude also provides `defn` for a function definition, `the` for a typed value, `->` and `->>` for threading a value through calls, `let-try` and `try->` for propagating errors, and `assoc` and `assoc-in` for changing a value. `defn` with a plain parameter list expands to `def` and `fn`. `->` passes the value as the first argument of each call, and `->>` as the last.

### Keyword parameters

A `defn` parameter list whose first element is a keyword declares keyword parameters, read like `defstruct` fields. Each `:name` takes an optional type, so `(:x :y)`, `(:x i64 :y i64)` and a mix declare two parameters. A type is never a keyword. The body names each parameter without the `:`. A list headed by `defaults` is always the defaults clause, even right after a keyword, and it comes last, as in `defstruct`. Any other parameter list keeps its plain meaning.

```lisp
(defn add (:x i64 :y i64) (+ x y))

(defn greet (:name str :greeting (defaults :greeting "hello"))
  (str greeting ", " name))
```

```rust
fn add(x: i64, y: i64) -> i64 {
    x + y
}
```

`defn` defines a positional function under a hygienic name, and a macro `add` that reads each call with the constructor's rule, with parameters in place of fields:

1. `:key value` pairs that name distinct parameters and every parameter without a default are a call by name. `(add :y 7 :x 6)` emits `add(6, 7)`, and `(greet :name "ana")` emits `greet("ana", "hello")`.
2. Other `:key value` pairs are positional only when the call has one argument per parameter and every keyword at an even position lands on a parameter whose written type is a function type. With `(defn sort-users (:key (fn ((ref user)) (ref str)) :label str) ...)`, `(sort-users :name "by name")` passes the `:name` closure as `key`. A parameter with no written type does not count as a function type.
3. Any other `:key value` pairs are a call by name, and the call is an error that names the missing, unknown or repeated parameter: `(add :x 6)` is "missing parameter `y` in `add`", `(add :z 6 :y 7)` is "`add` has no parameter `:z`", and `(add :x 6 :x 7)` is "`:x` is given twice in `add`".
4. Any other call with exactly one argument per parameter is positional. `(add 6 7)` emits `add(6, 7)`. Any other call is an error that names the parameter count.

A call by name always passes the arguments in parameter order. When it reorders the arguments, each argument that is a list binds to a temporary first, in the order written, so `(add :y (next-y) :x (next-x))` emits `{ let y: i64 = next_y(); let x: i64 = next_x(); add(x, y) }`. A literal or a name passes as written. A default is inlined at each call that omits it.

In value position, `add` expands to the positional function, so `(let plus add)` emits `let plus = add;`. The positional function takes the function's own Rust name, because the macro emits no item. A call to `add` in its own body expands like any other call, so a call in tail position still emits a `loop`. `(export add)` exports the macro, and an importing module calls `add` by name or by position. A `(type add ...)` signature names the positional function's type, with the parameters in declared order.

The prelude builds three tools on top of `macro`:

| Tool | Example |
|---|---|
| `defmacro` | `(defmacro unless (test body ...) (template (if (unquote test) (do) (do (splice body)))))` |
| `template` | `(template (def (unquote name) (splice body)))` |
| `defrule` | `(defrule unless ((test body ...) (if test (do) (do body ...))))` |

A `defrule` clause is a pattern and a template. In a pattern, `name ...` matches zero or more forms, at any depth, and `(quote name)` matches the symbol `name` itself. A `defmacro` parameter list is a pattern in the same language.

Write a macro with `defrule` when its expansion follows from the shape of its call. Use `defmacro` only for a macro that computes, such as one that counts forms or builds names. `dotimes` and `case` need no computation:

```lisp
(defrule dotimes
  (((index count) body ...) (for-each (range 0 count) (fn (index) body ...))))

(defrule case
  ((value clause ...) (do (let subject value) (case-clauses subject clause ...))))
```

One implementation per name keeps the compiler small. Compile-time twins of the prelude's Rust templates would disagree with Rust on integer width, overflow, division by zero and floats. Most macros need no computation when written with `defrule`.

### The `code` module

The compiler provides the `code` module for macro bodies. Code that uses it exists only at compile time.

| Built-ins | Purpose |
|---|---|
| `first rest list prepend concat length empty? nth take drop` | Build and take apart lists of forms. |
| `symbol? keyword? list? string? number?` | Test what kind of form a value is. |
| `symbol-name`, `keyword-name`, `keyword-field`, `field-keyword`, `(symbol context part...)`, `(string part...)` | Read and build names and strings. |
| `escape-string`, `escape-char`, `escape-format`, `string-strip-suffix`, `string-strip-prefix` | Escape a string for a Rust string literal, a Rust character literal or a format string in a template, and drop a suffix or a prefix. |
| `(same-binding? left right)` | Test whether two symbols refer to the same binding. |
| `(fail "message" form)`, `(fail "message" form "help")` | Stop expansion with an error at `form`, and an optional help line that says how to fix the call. |
| `number-add number-sub number-lt number-le number-gt number-ge` | Count and compare positions in code. They take integers. |
| `value-eq bool-not` | Compare forms and negate booleans. |

A keyword is a symbol whose name is `:` followed by at least one character, such as `:year`. `keyword?` tests for one, and `keyword-name` returns its name without the `:`, so `(keyword-name (quote :year))` is `"year"`. `keyword-field` returns the field symbol, so `(keyword-field (quote :year))` is `year`, with the keyword's location and hygiene, so an error about the field points at the keyword the user wrote. `field-keyword` is its inverse: `(field-keyword (quote year))` is `:year`, with the field's location and hygiene. The compiler states the `:` marker once, so macro code never compares names with it.

Macro code that maps a function over a list of forms imports `map-list` with `(use prelude/macros map-list)`.

### Hygiene

Macros are hygienic. The expander uses sets of scopes:

- Every symbol carries a set of scopes.
- A module, a `fn` and the rest of a body after a `let` each add a fresh scope to the forms inside them.
- Each macro call adds a fresh introduction scope to its input and flips it on its output, so only the symbols the macro introduced carry it.
- A macro call in a definition context also adds a use-site scope to its input. Binders that a definition introduces lose their use-site scopes.
- A reference resolves to the binding with the same name and namespace whose scope set is the largest subset of the reference's scopes.
- `quote` removes the scopes of enclosing `fn`, `let` and `where` forms and of definition-context macro calls from its datum. Quoted names in different helper functions can then refer to each other, and a macro's local variables never leak into the code it returns.

Hygiene gives three guarantees:

1. A binding that a macro introduces never captures a user's binding, and a user's binding never captures the macro's.
2. A free symbol in a macro's output means what it meant where the macro was defined.
3. A name that comes from the macro's input binds in the user's scope.

A `rust` template's own binders are hygienic too. A name written as `{!name}` takes a fresh Rust name at each use, so a template's closure parameter never hides a name that the program's code captures. The template keeps the name when no name at the call uses it, so `numbers.sort_by(|left, right| ...)` reads as hand-written Rust.

`(symbol context part...)` builds a symbol with the scopes of `context`. It is the one deliberate way to introduce a name the user can see:

```lisp
(defstruct point :x f64 :y f64)   ; defines ->point
```

Field names are not hygienic. They are looked up through the struct type.

## Modules

- One file is one module. `(use text/format pad-left)` loads `text/format.rslisp` relative to the entry file's directory and binds `pad-left`.
- A name is private to its file until an `(export ...)` lists it. `export` can list imported names, which re-exports them.
- Exporting a struct makes all its fields public: every module that imports it can read, build and update it, and its emitted Rust fields are `pub`.
- Every user module imports the prelude. An explicit `use` shadows a prelude name.
- Every prelude module imports the kernel.
- Import cycles are an error.

## Rust interop

`rust` binds a name to a piece of Rust:

```lisp
(rust-trait number "crate::rt::Number" i8 i16 i32 i64 u8 u16 u32 u64 f32 f64)
(def add (rust "{0} + {1}" (where ((n number)) (fn (n n) n))))
(def not (rust "!{0}" (fn (bool) bool)))
```

A `rust` template names a type parameter of its `where` by name, as `{target}` in `"{0}.parse::<{target}>()"`. The emitter fills it with the Rust type that the binder takes at that call. The name must be a binder of the template's `where`; a name with no matching binder is an error.

`rust-type` binds a type name to a Rust type. `{0}` in the template is the first type parameter in the head, as `{0}` in a `rust` template is the first argument. A `rust-type` may also list clauses after its template. A clause is a type and a template, and it applies when the type's first argument equals the clause's type. Every other argument uses the template after the name. A `rust-type` with clauses takes exactly one type parameter.

```lisp
(rust-type (boxed item) "Box<{0}>")
(def box-new (rust "Box::new({0})" (where (a) (fn (a) (boxed a)))))

(rust-type (non-zero n)
  "std::num::NonZero<{0}>"
  (u8 "std::num::NonZeroU8"))
```

`rust-type` is a kernel form: only `struct` creates a type, and it cannot name a type that Rust defines. A template that starts with `impl ` is opaque. A closure parameter or a `let` writes an opaque type as `_`. A `const`, a `static` or a function-type parameter cannot hold one, and rustc reports it. A struct whose Rust name equals the first name in a used Rust type's template is a name collision.

A `rust-trait` implementor can take type parameters from a `where`. `(where ((a shown)) (ref a))` implements the trait for a reference to any type that implements it. An entry `(with type type)` gives the trait's second type: `(with string str)` means `String: PartialEq<str>`. A type parameter alone cannot be an implementor, because it would match every type. A struct can appear in a `rust-trait` only for a trait its emitted `derive` lists.

A `rust-trait` path that holds `{0}` is a Rust `where` predicate, not a trait path. `(rust-trait step "std::ops::Range<{0}>: Iterator<Item = {0}>" i64 ...)` makes a bound `(n step)` emit `where std::ops::Range<N>: Iterator<Item = N>` on the function, with the type parameter in place of `{0}`. A generic caller inherits the bound. The predicate reaches function items only, not trait or impl methods.

A `rust-trait` declares an associated type with `(associated (name owner) "RustName")`, and may declare several. The clause names its one parameter, the implementor, on its name, as a type definition does. Each entry then ends with one `(name type)` clause per associated type, inside a `where` whose parameters the type may use:

```lisp
(rust-type parse-int-error "std::num::ParseIntError")
(rust-trait from-str "std::str::FromStr"
  (associated (parse-error target) "Err")
  (where () i64 (parse-error parse-int-error)))

(rust-trait add "std::ops::Add"
  (associated (output left) "Output")
  (where () (with i64 i64) (output i64)))
```

The associated type is a type constructor of one argument, the implementor: `(parse-error i64)` is `parse-int-error` wherever it appears, in inference, in diagnostics and in the emitted Rust. A projection whose argument is not yet known stays a projection until inference learns it. A projection on a type parameter stays a projection and emits Rust's qualified form with the parameter's bound, so `(output a)` under `(a (add i64))` emits `<A as std::ops::Add<i64>>::Output`. The parameter's `where` must bind the trait. An entry that leaves out a declared associated type, a clause that names a type the trait does not declare, and a projection on a type that does not implement the trait are errors. An associated type needs the compiler because only the compiler can replace a projection by its type during inference.

| Template rule | Effect |
|---|---|
| Placeholders | `{0}`, `{1}` and so on take the call's arguments. `{&0}` takes an argument that Rust borrows by itself, as a format macro or a comparison operator does. No `&` is written before it, and the call does not move it. `{target}` is the Rust type that the `where` binder `target` takes at the call. `{!name}` is a name that the template's text binds: every occurrence takes the same fresh Rust name, chosen not to hide a name at the call. `{{` and `}}` are literal braces. |
| Inlining | A `def` of a `rust` form emits nothing. Each use inlines the template. |
| Parentheses | A non-atomic argument is parenthesized unless its placeholder is the whole template, sits between `(` or `,` and `,` or `)`, or Rust's precedence and associativity keep its grouping on the side it takes. A prefixed argument such as `*x` is parenthesized only before `.`, `[`, `(` or `?`. |
| Deref place | A `*` written immediately before a placeholder is dropped when the call borrows a `mut` place, because the `*` and the borrow cancel. |
| Evaluation order | A repeated or out-of-order placeholder binds its arguments to temporaries first. |
| Values | A function template used as a value becomes a closure whose parameters carry the types its signature declares. A bare path is used as the path. |
| Runtime | `prelude/runtime/` holds the parts of `mod rt`. A program carries only the parts its code names, and its `Number` trait carries only the literal conversions it calls. |
| Unsized | A type parameter used only behind references is declared `?Sized` exactly when a reached instantiation is unsized, so a caller that passes `str` can fill it. |

### Formatting

The emitted Rust is formatted by `rustfmt` when `rslisp emit` or `rslisp build` writes it, not by a compiler stage. `rustfmt` reads the nearest `rustfmt.toml`, so the program's directory chooses the layout. If `rustfmt` is missing or fails, the command prints one note and writes the unformatted Rust.

## Names in Rust

| rslisp | Rust |
|---|---|
| `make-point` | `make_point` |
| type `point` | `Point` |
| `admin?` | `is_admin` |
| `add-item!` | `add_item` |
| `point->string` | `point_to_string` |
| constant `admin?` | `IS_ADMIN` |
| other punctuation | spelled out, so `+` becomes `plus` |
| a Rust keyword | a raw identifier, so `type` becomes `r#type` |
| a program type or trait that hides a Rust prelude name | suffixed with `2`, so `sized` becomes `Sized2` |
| a local whose name a binding still in scope uses | numbered, so `n` becomes `n_2` |
| an item a macro introduced whose name a written item takes | the next free number, so the positional function of `car` becomes `make_car_2` beside a program's own `make-car`, and a struct or enum takes the number with no underscore, as `Point2` |

A program type or trait that mangles to a name Rust's 2021 prelude brings into
scope takes the suffix `2`, so the emitted name never hides `Sized`, `Clone`,
`Iterator`, `Vec` or another prelude name. A `rust-type` or `rust-trait` path is
never renamed.

An item a macro introduced yields its Rust name to an item the program wrote, whatever the order of their uses. The introduced item takes the next number that no other item of its module takes. `rslisp check` and `rslisp signatures` print an introduced function under the name of the macro whose value position expands to it, so the positional function of `car` prints as `->car`. They leave out an introduced function that no such macro names.

## Status

| Milestone | Status |
|---|---|
| 1. Scaffold | done |
| 2. Reader and diagnostics | done |
| 3. Expander, hygiene and interpreter | done |
| 4. Modules | done |
| 5. Types | done |
| 6. Emit and build | done |
| 7. Ownership | done |
| 8. Generic types | done |
| 8.1 References at call sites | done |
| 8.2 Struct traits | done |
| 8.3 Macro code | done |
| 8.4 Loops | done |
| 9. Enums and `match` | done |
| 10. User-defined traits | done |
| 11. Written lifetimes | done |

A planned milestone is settled design, and new work follows it.

Unless a program writes ownership, traits, bounds or loops, the compiler infers them, and a written form is exact. A `type` signature is exact: a type parameter it declares without `copy` gets no inferred `copy` bound. A `derive` form lists a type's traits exactly. A written `ref`, `mut` or `deref` turns off adjustment for that value.

### 8.1 References at call sites

A value meets an expected type in four places:

- an argument
- a return value
- an annotation
- a function passed as a value

At each of them, the compiler adds one borrow, removes borrows, or copies a Copy value out of a borrow. It never clones. It decides each adjustment after inference, when every type is known.

- The rule applies to the outer reference only. `(vec (ref person))` never becomes `(vec person)`.
- A `(ref (vec a))` value meets a `(ref (slice a))` type as Rust's deref coercion does. The compiler writes no new syntax, and a written `(ref (vec a))` stays `&Vec<T>`.
- An inferred parameter of a vector type takes the slice when no use needs the vector. A use that takes a written `(ref (vec a))`, makes a `mut` borrow, owns the value or uses it in any other position keeps the vector. An inferred `string` parameter takes `str` when no use needs the owned string.
- In a generic slot, a borrowed Copy value of 16 bytes or less is copied. A larger value stays borrowed. Parameter modes use the same limit.
- A written `ref`, `mut` or `deref` turns the rule off for that value. The compiler reports a mismatch as written.
- A `mut` borrow always takes the original place, never a copy or a temporary.
- If a function passed as a value needs an adjustment of its parameters, the compiler wraps it in a closure.
- If a body copies a generic parameter, the compiler infers the parameter's `copy` bound.
- If the name of a function that changes an argument does not end with `!`, the compiler warns. The emitted Rust drops the `!`.
- `deref` copies a Copy value out of a borrow.
- A command prints each function's inferred signature, so that a `type` form can pin it.

Rust adjusts a receiver's references automatically, but rslisp has no receivers, so the rule covers every call. A Copy value of 16 bytes or less travels in registers, so copying it costs no more than borrowing it.

### 8.2 Struct traits

Every struct implements each of `clone`, `debug`, `partial-eq`, `eq` and `hash` that all its field types implement. If a struct's fields are all Copy and the program never changes the struct in place, the struct is `copy`. The program changes a struct in place when a `mut` parameter receives it or one of its fields changes.

- `(derive name trait...)` lists a struct's traits exactly and turns inference off for that struct.
- A written list can add `partial-ord`, `ord` or `default`, drop an inferred trait, or pin the set.
- A listed trait brings the traits Rust requires of it. `ord` brings `partial-ord`, `eq` and `partial-eq`.
- If a field blocks a listed trait, the `derive` form is an error.
- The compiler reports a missing trait at its use and names the field or `derive` form that blocks it.
- `prn` and `pr-str` print a value's data form. `println` keeps printing user-facing text.
- The signature command also prints each struct's inferred `derive` form.
- `derive` is a kernel form, because the prelude cannot add attributes to an emitted struct.

Python, Clojure and Kotlin give records equality and printing by default. An order or a default picks a meaning, so the program writes it. A struct that the program changes is never copied, so a change cannot land on a copy.

### 8.4 Loops

A call to the enclosing function in tail position already compiles to a Rust `loop`, as the Bodies section says.

- A written `(recur argument...)` compiles to the same loop.
- `recur` is an error outside tail position.
- The number of arguments to `recur` must equal the number of parameters of the target.
- `recur` targets the nearest `fn`, so the prelude defines Clojure's `loop` as a `defrule`.
- Mutual recursion stays recursion.
- `recur` is a kernel form, because the prelude cannot make a function jump back to its start.

Rust has no guaranteed tail calls. Without this milestone, recursion is the only way to loop, and a deep loop overflows the stack.

`loop` binds names to values, then repeats its body with `recur`. It emits as a Rust block and an inline `loop`:

```lisp
(loop ((index 0))
  (if (< index 10) (recur (+ index 1)) index))
```

```rust
{
    let mut index: i64 = 0;
    loop {
        if index < 10 {
            index += 1;
            continue;
        } else {
            break index;
        }
    }
}
```

### 9. Enums and `match`

An `enum` declares a sum type. A variant is a bare name, `(name type ...)` with positional payloads, or `(name (struct (field type)...))` with named fields. Within one variant the payload is either types or one `struct` body. A generic enum puts its parameters on its name, as `(enum (pair-of a) (both a a) neither)`.

An optional `(rust "Path")` as the first form after the name makes the enum name a Rust type, such as `(enum (option value) (rust "Option" bare) (some value) none)`. The clause holds one string, a Rust path, or a type template when the string holds a placeholder such as `{0}`. An optional word `bare` after a path says the variants are in Rust's prelude. Only in that position does `(rust "...")` mean the clause. The compiler appends the enum's type parameters to a path, so `(option i64)` emits `Option<i64>`. Such an enum emits no Rust definition. A construction or pattern emits the path, then `::`, then the variant's Rust name, as `(option/some x)` emits `Option::Some(x)` and the pattern `none` emits `Option::None`. A `bare` enum emits the variant's Rust name alone, as `(option/some x)` emits `Some(x)` and the pattern `none` emits `None`. The compiler reads `bare` from the clause; it does not name Rust's prelude types or variants itself. The enum reads as a `rust-type` in its emitted Rust, its traits, its size, its display in diagnostics and `signatures`, and `try`. Its traits come from `rust-trait` entries only, with no inferred traits and no `derive`; a `derive` of it is an error, `L0435`.

A template clause fills its placeholders from the enum's type arguments and from the variant's payload. Such an enum has exactly one variant, with one positional payload part per placeholder; otherwise the compiler reports `L0438`. The type emits the template filled with the type arguments, so `(pair usize string)` emits `(usize, String)`. A construction and a pattern emit the template filled with the payload in order, so `(pair i x)` emits `(i, x)` and the pattern `(pair index line)` emits `(index, line)`.

```lisp
(enum shape
  (circle f64)
  (rect f64 f64)
  (labeled (struct (text string) (size f64)))
  point)
```

A construction is qualified. `(shape/circle 1.0)` constructs a payload variant, and `shape/point` is the value of a variant with no payload. A variant with a struct payload takes its fields positionally, in field order: `(shape/labeled (to-string "box") 2.0)`. Using a payload constructor as a value is an error. `(export shape)` exports the enum and every constructor it defines. The prelude defines `ok` and `err` as macros over `result/ok` and `result/err`, and `some` as a macro over `option/some`, so a program writes `(ok value)`, `(err error)` and `(some value)`. `none` is the `option/none` value.

An enum gets the traits of a struct under 8.2, with its payloads in place of fields. Each structural trait that all payloads support is inferred, and `copy` is inferred when nothing changes the enum in place. A function-typed payload allows `clone`, `copy` and `debug`, and blocks comparison and hashing, because Rust compares function pointers unreliably. A written `derive` is exact, and an order follows the declared variant order. A blocked trait names the variant whose payload blocks it.

A payload that holds its own enum by value, directly or through another type that stores it by value, is stored in a `Box`. The compiler boxes the payload position, writes `Box::new` at construction, and a pattern binds the payload as if it were unboxed. A payload whose cycle passes through a `rust-type`, such as `(vec type)`, is not boxed: the compiler does not look through a Rust type. A written `(box type)` is the exact form and stops inference for that position. A value cycle adds one box.

`signatures` prints an `(enum ...)` line with the inferred boxes for each enum that has one.

A nested pattern at a boxed position emits a guard and a destructuring `let`. The arm pattern binds the boxed value to a fresh name, as a peeled binding does, and the guard tests the pattern with `matches!`. The tested pattern writes each binding as `_`, so it tests only the shape; a boxed position inside it binds another fresh name and nests its own `matches!`, and a string literal adds its comparison. The arm's prologue then destructures the pattern from the peeled value, with `let <pattern> = <value> else { unreachable!() };`, or a plain `let` when the pattern is sure. A pattern at a boxed position that is a binding or `_` keeps the peel instead.

rustc counts a guarded arm as covering nothing. The type stage records whether the arms without a guard cover every value, and the emitter adds a last `_ => unreachable!()` arm only when they do not.

Some recursive types still fail. A recursive struct, and a recursive enum whose cycle passes through a `rust-type` that holds its argument by value, reach rustc, which reports an infinite type as `L0604`. An enum whose type parameter is used only inside references to the enum itself, at any depth, is an error, `L0439`.

A `match` tests one value against patterns:

```lisp
(match shape
  ((circle r) (* 3.14 r r))
  ((rect width height) (* width height))
  ((labeled _ size) size)
  (point 0.0))
```

An arm is one pattern and one body expression. The body is in tail position, so `recur` works in an arm, and a call to the enclosing function in an arm compiles into the enclosing tail loop. When one arm borrows a small Copy value and another arm gives the value, the match copies the borrow out, as a call argument does. A pattern is `_`, a literal, a bare name, `(variant pattern...)`, or `(type-name pattern...)`, and patterns nest. Literal patterns are numbers, booleans and strings.

A struct pattern is `(type-name pattern...)`, with exactly one pattern per field in field order. A wrong count is the wrong-number-of-pattern-parts error, as for a variant. A struct pattern nests inside a variant or pair pattern, and either of those nests inside it. An owned matched value moves each bound field out, and a `ref` or `mut` matched value binds a `&` or `&mut` field, as for every other pattern.

A pattern is sure when no value can fail it: `_`, a bare name that binds, a struct pattern whose parts are sure, or a variant of a one-variant enum whose parts are sure. A `match` with one sure arm emits `let <pattern> = <value>;` and then the arm's body, in place of a Rust `match`. A struct pattern therefore emits a `let` that names the fields it binds and writes `..` for the fields it skips with `_`, as in `let Parsed { node, .. } = p;`.

`let-fields` takes `(let-fields (type-name (name ...)) value body ...)` and expands to a one-arm `match`, as `let-pair` does. Each name is a field pattern in field order, and `_` skips a field:

```lisp
(let-fields (parsed (node _)) p
  (tree/branch node))
```

A `string` value matched against string literals emits `.as_str()` on the value, which borrows and allocates nothing. A string literal nested in a pattern, at a position of type `string`, binds a fresh name and adds an `if` guard that compares the name with the literal, as `Some(Token::Symbol(text)) if text == "("`. Several such literals in one pattern join their guards with `&&`. A position of type `str` matches the literal directly.

A variant name in a pattern is short, as in `(circle r)`. Its enum is the type of the value at that position. When the matched value's type is still unknown, the enum is the one visible enum that has every variant name the arms use. If two visible enums fit, the match is an error that asks for one qualified pattern. A qualified pattern, such as `(shape/circle r)`, is always allowed.

A bare name that names a variant with no payload of the matched enum is that variant. Any other bare name binds the value. A variant pattern binds no value, so in its arm the name keeps its meaning from outside the `match`. A variant with a `struct` payload takes its patterns positionally, in field order.

When the matched value is a reference, each name a pattern binds is a reference of the same kind, as Rust's default binding modes do.

A `match` must cover every value its type can hold. A missing value is an error that lists each missing case as a pattern. An arm that no value can reach is a warning that points at the earlier arm covering it. Numbers and strings are never covered by literals alone, so a match on them needs a `_` or a name arm. The compiler decides coverage with the usefulness algorithm from Maranget's "Warnings for pattern matching", so nested patterns are checked too.

`match` exists only in program code. In macro code it is an error, because a macro cannot run a value's variants.

`enum` is a kernel form, because only the compiler can create a Rust type and its variant bindings. The prelude cannot express a sum type, and a macro cannot register variants for construction or traits.

`match` is a kernel form, because only the compiler can bind a payload without a clone and knows an enum's variants. A prelude function receives an enum as an opaque value, so it cannot take a variant apart or choose an arm. A macro could expand to a Rust `match`, but it would have to restate every variant and it could not bind a borrowed payload without writing a clone.

### 9.1 Propagating errors with `try`

`(try expression)` emits `expression?`. The value is a `result` or an `option`. An `option` needs the enclosing function to return an `option`. A `result` needs the enclosing function to return a `result` whose error is the value's error, or whose error is a program enum with exactly one variant that holds the value's error. The compiler emits `impl From` for each conversion a `try` uses, so `(try x)` emits `x?` and Rust converts the error. The enum takes no type parameters, and the variant holds one positional payload that is not a box. A second variant that holds the same error makes `try` report `L0437`, because the conversion cannot choose between them.

A `try` conversion needs the function's error type to be known. An unwritten signature fixes the error type to the first `try`'s error, so a function that converts errors writes a `type` signature naming the error enum. A conversion moves the error into the variant and adds no context; `map-err` stays the way to add context.

When a `try` converts no error and it is the whole argument of the `result` enum's `ok` constructor, `Ok(x?)` has the same value as `x`. The compiler emits `x` instead, so `(defn half (text) (ok (try (parse text))))` emits `text.parse::<i64>()`. A `try` that converts an error keeps `Ok(x?)`.

Conversion belongs to the compiler: only type inference knows the enclosing function's error type, and only the compiler emits a Rust `impl From`. A macro sees neither the function's inferred error nor an enum's variants.

`try` targets the nearest `fn`, as `recur` does. Inside a lambda it leaves the lambda, as Rust's `?` leaves a closure. A `try` outside every `fn` is an error, and in macro code it is the compile-time error for a form that only exists at runtime.

Programs read functionally through two prelude forms over `try`, as `loop` is over `recur`:

```lisp
(let-try ((tokens (tokenize source))) (evaluate tokens))
(try-> source tokenize evaluate)
```

`let-try` expands each binding to `(let name (try expression))` and then the body. `try->` threads a value through calls and applies `try` to every step except the last: `(try-> x f g)` is `(g (try (f x)))`, and `(try-> x f)` is `(f x)`. `ok-or` turns an `option` and an error value into a `result`, and `map-err` converts an error.

`try` is a kernel form, because a prelude function cannot leave its caller early.

### 10. User-defined traits

A program defines traits and implements them. An `impl` names a struct, an enum, a primitive such as `i64` or `bool`, `string`, or an applied type such as `(vec i64)`. An unknown name in the implementor is a type variable, so `(impl sized (vec a) ...)` implements `sized` for every vector and emits `impl<A> Sized2 for Vec<A>`. A bare type variable is an error, because an `impl` must name a type around it. The compiler infers the bounds on an impl's type variables from the method bodies and emits them on the impl head, as `impl<A: Clone> Sized2 for Vec<A>`. A written bound waits for a program that needs one. A trait declares an associated type with `(type item)`. A method's type names it as `item`, and the trait emits `Self::Item`. An impl states it with `(type item a)`, and every associated type the trait declares must appear exactly once in the impl. A bound `(c (container item))` means `C: Container<Item = item>`. Associated types need the compiler because only the compiler adds the trait item, infers the implementor's type and emits `type Item = ...`. An unsized implementor such as `str` must take every receiver by reference. `rust-trait` binds only traits that Rust defines.

A `rust-trait` that declares a method can also be implemented, and the compiler emits the Rust `impl`. The method's name and signature come from the `rust-trait` entry, so the program writes only the method body. Its implementor must be a struct or enum of the program, because Rust's coherence rule rejects an implementation for a type outside the crate. A `rust-trait` entry may also state a `(help "text")` line, which the compiler shows when a type does not implement the trait.

The prelude's `display` trait declares `fmt`, and the prelude's `display` macro writes the implementation from printing parts:

```lisp
(struct point (x i64) (y i64))
(display point (p) "(" (field p x) ", " (field p y) ")")
```

The parts follow `println`: a string is literal text, any other form prints with `display`, and `(str ...)` splices. A `(pr-str ...)` part prints its value's data form. When the parts are exactly one `match` form, each arm body becomes a write of that body's parts, and a `(str ...)` arm writes those parts. Any other arm body is one part, and the rule applies again to a nested `match` or `if`. A single `if` form writes in both branches the same way. The type then works with `print`, `println`, `str`, `to-string`, and as the error type of a `main` that returns a `result`. An implementor that is not a struct or enum of the program is an error, `L0445`.

```lisp
(trait shape
  (area (fn ((ref self)) f64))
  (scale! (fn ((mut self) f64) unit)))

(impl shape circle
  (area (c) (* 3.0 (field c radius) (field c radius)))
  (scale! (c factor) (do)))
```

`self` is a type variable bound by the trait. A method is an ordinary function type whose first parameter states the receiver's ownership: `self`, `(ref self)` or `(mut self)`. Such a method emits a Rust receiver, `fn area(&self) -> f64`, and a call emits `c.area()`. Every other method has no receiver and emits an associated function, `Shape::name(...)`. A method value such as `(map shapes area)` emits a path, `Shape::area`. A method with a body after its type has a default. An impl may leave that method out, and the body emits inside the Rust trait item; an impl that writes the method overrides the default. A trait head that is a list holds the trait's name and then its supertraits: `(trait (shape (display) (clone)) ...)` emits `trait Shape: std::fmt::Display + Clone`. Every implementor must implement each supertrait, and the compiler reports a missing one at the impl. A default method and a supertrait need the compiler because a macro cannot add a body or a bound to a Rust trait item.

An `impl` writes each method like `defn`. Its parameter types come from the trait, and the first parameter emits as Rust's `self`. A trait signature owns its lifetimes: the compiler adds the shared lifetime there, and an impl copies the parameter types exactly, except that a method without a receiver narrows the shared lifetime to the parameters whose borrows reach its result. An impl for a type that holds a borrow writes the type with its lifetime, as `impl<'a> Finder for Snippet<'a>`, and its methods elide every lifetime Rust elides. A missing method, an extra method and a method the trait does not declare are errors. An `impl` may name a trait and a struct or enum from any visible module. Every module of a program is one Rust crate, so Rust's coherence rule never rejects such an `impl`. A method name is a top-level name, so two traits in one module cannot share one; `use` picks between modules.

`(export shape)` exports the trait and every method it declares, as `(export shape)` exports an enum and its constructors. `(use shapes shape)` imports the trait and every method it declares. A method name that clashes with a name the module already binds is the existing duplicate-name error.

A trait emits in its own module's Rust `mod`, with the visibility the emitter gives an exported struct. An `impl` emits in the `mod` of the module that writes it. A module that calls a method with Rust method syntax has the trait in scope: the compiler emits a Rust `use` of the trait at the top of that module's `mod`, once per trait and only in a module that needs it. A method value emits the trait's path from the calling module, as `Shape::area` with the `use` or as a full path without one.

A method is an ordinary function: `(area c)` is a call, and `(map shapes area)` passes the method as a value. A call on an unknown type adds the trait's bound, as `<` adds `ord`, and a written signature stays exact.

In a type position, `(box shape)` where `shape` is a trait emits `Box<dyn Shape>`. A `(box-new value)` whose expected type is `(box shape)` emits the coercion to the trait object, and the only allocation is the `box-new` the source writes. A boxed value calls the trait's methods as any value does, as `(area shape)` does. A trait is boxable when every method takes `(ref self)` or `(mut self)`, no method is generic, no method returns `self` by value, and the trait declares no associated type. A trait that is not boxable is an error at the `(box ...)` type, `L0448`. A trait object needs the compiler because only the compiler knows a trait's implementors and emits the coercion.

`trait` and `impl` are kernel forms. A macro returns a form and cannot emit a Rust `trait` or `impl` item, and only the compiler can add a trait and its implementors to the trait table. A method call needs no kernel form: each method is an ordinary generic function, so the existing call, inference and emission paths handle it.

A `rust-trait` method needs no kernel form beyond the `impl` item it lets the program write. The prelude states the method's name and signature, and the compiler reads them from the `rust-trait` binding, so no Rust fact about the trait is restated in Rust. The `display` form is a prelude macro over `impl`, because a macro returns a form and cannot emit the Rust `impl` item itself.

### 11. Written lifetimes

A function whose result holds a borrow adds one lifetime when its parameters contribute more than one elided lifetime. Every reference parameter and the result share it:

```lisp
(type longer (fn ((ref str) (ref str)) (ref str)))
(defn longer (a b) (if (> (len a) (len b)) a b))
```

```rust
fn longer<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() > b.len() { a } else { b }
}
```

A written `type` signature with references but no lifetimes infers them the same way. Written ownership stays exact. When the parameters contribute one lifetime, Rust's elision decides, and no lifetime appears.

The compiler infers the parameter modes as well. A parameter that shares one type with another takes the value its argument refers to, not the argument's reference, so `(defn longer (a b) ...)` needs no `type` signature.

A program writes a lifetime when it wants the result to borrow from one particular parameter:

```lisp
(type first (where ('a 'b) (fn ((ref 'a str) (ref 'b str)) (ref 'a str))))
(defn first (a b) a)
```

```rust
fn first<'a>(a: &'a str, b: &str) -> &'a str {
    a
}
```

A lifetime is declared with the type variables, as `(where ('a 'b) ...)` does. A reference names it as its first part, `(ref 'a str)` or `(mut 'a point)`. A written lifetime is exact, and the compiler adds no shared lifetime to a type that writes one. A lifetime that links positions, as `'a` links the first parameter and the result, stays named in the emitted Rust. One that appears once and nowhere in the result emits as Rust's elided form, so `b` in `first` above becomes `b: &str`.

A struct whose field stores a borrow gets one inferred lifetime parameter before its type parameters. The source keeps writing the bare type name, and the compiler fills the lifetime at each use:

```lisp
(struct snippet (text (ref str)))

(enum token
  (symbol (ref str))
  (number (ref str)))
```

```rust
struct Snippet<'a> {
    text: &'a str,
}

enum Token<'a> {
    Symbol(&'a str),
    Number(&'a str),
}
```

A use of a borrowed struct emits `Snippet<'_>` or the function's shared lifetime. A lifetime written in the head is exact and replaces the inferred one, as `(struct (label 'a) (text (ref 'a str)))` does.

A trait signature owns its lifetimes. The compiler adds the shared lifetime to the trait's declaration, and an impl copies the parameter types exactly, except that a method without a receiver narrows the shared lifetime to the parameters whose borrows reach its result. An impl for a type that holds a borrow writes that type with its lifetime, as `impl<'a> Finder for Snippet<'a>`, and its methods elide every lifetime Rust elides. A method with a `(ref self)` receiver takes no shared lifetime, because Rust's receiver elision makes its result borrow from the receiver.

A returned borrow of a local value is an error, `L0510`, because the local is dropped when the function returns. The check follows the returned value through blocks, `if`, `match`, struct and enum constructions, and calls whose result borrows the argument. rustc reports the lifetime errors the compiler does not see, mapped back to the source and the names the program wrote.

`ref`, `mut`, `where`, `struct` and `enum` gained a lifetime position or binder, so this milestone needs no kernel form. A macro cannot add a lifetime to a function's emitted signature or to a struct's generic list, because the compiler builds both from the inferred type rather than from a form a macro returns.
