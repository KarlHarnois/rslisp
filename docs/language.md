# The rslisp language

`rslisp` is a placeholder name. Source files use the `.rslisp` extension. A source file may start with a UTF-8 byte-order mark, which the compiler skips, as rustc does.

The language is a Lisp that compiles to Rust. Its design follows five principles:

1. **User experience first.** The user's experience wins over an easier implementation in every trade-off. It includes the emitted Rust, diagnostics, run output, compile speed and predictable rules, so a uniform rule beats a special case.
2. **Few rules, no exceptions.** A feature that needs an exception gets redesigned or left out.
3. **Small compiler, big prelude.** Anything the language can express lives in the prelude, written in the language itself. The compiler only reads, checks, reports and emits.
4. **Rust's performance and Rust's model.** Ownership, borrowing, traits and generics work as in Rust. The compiler never adds a hidden clone or allocation, except the `Box` a recursive enum payload requires.
5. **Few special characters.** The reader treats only `(`, `)`, `"`, `;` and whitespace specially.

## Syntax

The reader knows four rules:

1. A file is a sequence of forms.
2. A form is an atom or a list: `( form* )`.
3. An atom is a string, a number or a symbol.
4. `;` starts a comment that runs to the end of the line.

`(comment form ...)` is the prelude's comment block, as in Clojure. Its forms must read, so their parentheses balance and their strings close, and `rslisp fmt` lays them out as code, but they never expand, check or run, so a `defn` inside one defines nothing. A file keeps scratch expressions in a `comment` to send one at a time to `rslisp eval` or the prompt. The call is `unit` at top level and in expression position, as Clojure's is `nil`, so `(+ 1 (comment 2))` is a type error. There is no `#_` form discard, because it would make `#` a character that the reader treats specially.

### Atoms

| Atom | Examples | Rule |
|---|---|---|
| String | `"hello"`, `"tab\there"` | Double quotes, with Rust's escapes: `\n \r \t \\ \0 \" \u{...}`. |
| Number | `42`, `-7`, `1_000`, `2.5`, `1e9` | A token that starts with a digit, or with `+` or `-` followed by a digit. Such a token must be a valid number. |
| Symbol | `point`, `make-point`, `admin?`, `+`, `...` | Every other token. |

There is no quote shorthand, no `#` syntax and no `[]` or `{}`. The reader rejects `[`, `]`, `{` and `}` so that code from other Lisps fails with a clear message.

A list nests at most 1,024 lists deep, a limit that `Form::NESTING_LIMIT` states. The reader reports a deeper list as `L0108` and skips it, so every later stage recurses only that deep. Macro output keeps the same limit: the expander reports an expression nested deeper than 1,024 expressions as `L0325`, because a macro can build forms deeper than its source. The forms that a top-level `do` splices into a module keep the same limit too: a form spliced through more than 1,024 nested `do` forms reports `L0325`, so a macro that recurses through a top-level `do` stops as it would inside a function. The macro calls inside one top-level form expand to at most 131,072 forms in all, a limit that `FormBudget::EXPANSION_LIMIT` states. When an expansion is a call of the kernel `def`, each argument that it returns unchanged as one of its elements counts once, so the prelude `def` spends only the forms it builds. The expander reports the call that passes it as `L0326`, so a macro whose expansion grows on every step, in one form or across many calls, stops with an error instead of running until it exhausts time or memory.

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
| `fn` | `(fn (parameter...) body...)` | `fn` item, closure, or block | params: binders. body: expressions. |
| `def` | `(def name value)` | `fn`, `const`, `static`, or nothing | name: binder. value: expression. |
| `let` | `(let name value)` | `let` | name: binder. value: expression. |
| `do` | `(do form...)` | block | forms: expressions. At module level: definitions. |
| `if` | `(if test then else)` | `if`/`else` | all: expressions. |
| `type` | `(type name type)` | signature | name: reference to a `def`, or to a macro whose value position expands to a function the macro introduced in this module. type: type. |
| `struct` | `(struct name (field type)...)` or `(struct (name parameter...) (field type)...)` | `struct` | name: type binder. params: type binders. fields: field labels with types. |
| `enum` | `(enum name (rust "Path")? variant...)` or `(enum (name parameter...) (rust "Path")? variant...)` | `enum`, or the named Rust type | name: type binder. params: type binders. `rust`: an optional Rust path. variants: a name, `(name type...)`, or `(name (struct (field type)...))`. |
| `match` | `(match value (pattern body)...)` | `match` | value: expression. patterns: a wildcard, a literal, a bare name, `(variant pattern...)` or `(type-name pattern...)`. arms: one body expression. |
| `derive` | `(derive type trait...)` | `#[derive(...)]` | type: reference to a `struct` or `enum` of this module. traits: references to traits. |
| `new` | `(new name (field value)...)` or `(new _ base (field value)...)` | struct literal or struct update | name: type, or `_` with a base expression. base: expression. fields: field labels with expressions. A label is a field name or a keyword, as in `(new car (:year 2025))`. |
| `assoc-at` | `(assoc-at value (key...) item)` | block | value: expression. keys: expressions. item: expression. |
| `map-at` | `(map-at operation value argument...)` | method call or block | operation: `get`, `keys`, `vals`, `iter`, `update` or `push`. value: expression. arguments: expressions. |
| `set-at` | `(set-at value (key...) item)` | assignment | value: expression. keys: expressions. item: expression. |
| `field` | `(field value name)` | `value.name` | value: expression. name: field label. |
| `ref` | `(ref value)` | `&value` | value: expression. |
| `mut` | `(mut place)` | `&mut place` | place: expression. |
| `deref` | `(deref value)` | `*value` | value: expression. |
| `quote` | `(quote form)` | none | form: data. |
| `macro` | `(macro (form) body...)`, the value of a `def` | none | form: binder. body: expressions. |
| `rust` | `(rust "template" type (type "template")...)` | the inlined template | type: type. clauses: a type and a template. |
| `recur` | `(recur argument...)` | `loop` | arguments: expressions. |
| `try` | `(try expression)` | `expression?` | expression: expression. |
| `infer` | `(infer expression)` | a string literal | expression: expression, checked and never run. |
| `rust-trait` | `(rust-trait name "Path" (method name type)? (structural key "text"...)? (help "text")? type...)` | trait bound, or a Rust `impl` for a program type when it declares a method | name: type binder. methods: a name and a function type whose first parameter may be `self`, `(ref self)` or `(mut self)`. structural: the Rust text of the impl the compiler writes for each struct and enum, under the keys `:struct`, `:field`, `:variant`, `:part` and `:bare`. help: the help line a missing implementor shows. types: types, `(with type type)`, or `(where (param...) type)`. |
| `trait` | `(trait name (method type)...)` | `trait` | name: type binder. methods: a name and a function type whose first parameter may be `self`, `(ref self)` or `(mut self)`. |
| `impl` | `(impl trait type (method (parameter...) body...)...)` | `impl` | trait and type: references to a trait and a struct or enum. methods: a name, parameter binders and a body. |
| `rust-type` | `(rust-type name "template" (type "template")...)` or `(rust-type (name parameter...) "template" (type "template")...)` | the filled template | name: type binder. params: type binders. clauses: a type and a template. |
| `use` | `(use module name...)` | path | module: module path. names: binders. |
| `export` | `(export name...)` | `pub` | names: references in every namespace. |

In type positions, the kernel provides `fn`, `fn-mut`, `fn-once`, `ref`, `mut`, `where`, `with`, `_`, and the primitive types `bool char str string unit i8 i16 i32 i64 i128 isize u8 u16 u32 u64 u128 usize f32 f64`.

In expression positions, the kernel provides `ref`, `mut` and `deref`, and the constants `true` and `false`. The constants are kernel constants, not prelude definitions. Macro code and program code then see the same booleans, so `if`, `and` and `or` work in both.

`ref`, `mut` and `deref` are kernel forms because a prelude function cannot mark its result as written. The compiler must see the written form to know that the value came from a written borrow or dereference.

`infer` is a kernel form because only the checker knows an expression's inferred type ([RFC 0054](../rfcs/0054-infer.md)). A prelude macro runs before the checker and sees forms, never types. A prelude function runs in the emitted program, where Rust's `type_name` spells Rust's names, runs its argument, and cannot name a type variable.

`assoc-at` is a kernel form because only the compiler knows a collection's inferred type. A prelude macro cannot choose between a vector and a map, so the `assoc-at` collection rule picks the emitted Rust by type. The rule knows `(vec a)`, `(sorted-map k v)` and `(hash-map k v)`.

`map-at` and `set-at` are kernel forms for the same reason. Only the compiler knows a map's key and value types, so one rule picks the emitted Rust for `get`, `keys`, `vals`, `iter`, `update` and `set!` from the inferred map type. `map-at` also carries `iter`: it accepts a vector, a slice or a map and emits `.iter()`, yielding `(ref a)` for a vector or a slice and `(pair (ref key) (ref value))` for a map. `get` accepts the same collections: it reads a vector or a slice by a `usize` index and emits `.get(index)`, returning `(option (ref a))`, and it reads a map by key. A collection whose type is still unknown is a slice for both. `get` as a value is the slice reader, of type `(fn ((ref (slice a)) usize) (option (ref a)))`, as vector `get` was before maps joined it. `push` adds an item to a vector, emitting `.push(item)`, or to a `string`: a `char` emits `.push(c)`, and any other item is a `(ref str)` and emits `.push_str(text)`. A target whose type is still unknown is a vector. A call that passes another collection to such a parameter is a type mismatch whose help names the assumption and suggests a `type` signature for the function. The prelude cannot express `iter`, `get` or `push` as a function, because one signature cannot accept both a slice and a map without an iterable trait, and only the compiler knows the collection's inferred type and the item type to yield. Rust has no trait method that covers `Vec::push`, `String::push` and `String::push_str`, so a trait would not give `push!` one template. The prelude macros expand to these forms, and the collection rules for `assoc-at`, `map-at` and `set-at` share the map type test.

### Module bodies

A module body is expanded in two passes:

1. Each top-level form is expanded in order until its head is a definition form: `def`, `type`, `struct`, `enum`, `derive`, `rust-trait`, `rust-type`, `trait`, `impl`, `use`, `export` or `do`. The names it defines are registered. A `def` whose value is a `macro` is evaluated right away.
2. `def` values, types, `impl` forms and `export` lists are expanded.

Values and types are therefore order-independent, like Rust items. An `impl` resolves its trait and its implementor in the second pass, so it may sit above the trait, the type or the `use` that it names. A macro must appear above its first top-level use.

### Bodies

A body is the sequence of forms in `fn`, `macro` and `do`. A `let` binds its name for the rest of the enclosing body. The value of a body is the value of its last form, or unit if the body is empty or ends with a `let`. A `(do)` before the last form has no effect and emits nothing.

A function that calls itself in tail position compiles to a Rust loop. The call reassigns the parameters that change, then starts the next iteration. A parameter to which the call passes a new value, such as a struct it builds or a local the loop body binds, takes the own mode unless it is a small Copy type, because a borrow of that value would not outlive the iteration: `(count-up limit (->counter :label (str "c") :count (+ (:count state) 1)))` emits `state = Counter { ... };` with `mut state: Counter`. A parameter to which the call passes itself, another parameter, or a field of one keeps the mode its uses need. A call to itself anywhere else stays recursive. A `fn` that the program calls where it is written emits as a block that binds each parameter with `let`.

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

A bound names the trait and, optionally, its second type: `(a (partial-eq b))` is Rust's `A: PartialEq<B>`. The second type must be a type the program knows or a binder of the same `where`, as in `(where ((a (partial-eq b)) b) (fn (a b) bool))`. A plain bound keeps Rust's default: `(a partial-eq)` is `A: PartialEq<A>`. So `(a partial-eq)` and `(a (partial-eq a))` are one bound, and each meets a need for the other. A trait takes a second type exactly when one of its `rust-trait` entries uses `with`.

A struct or enum whose fields or payloads store a borrow gets one inferred lifetime parameter before its type parameters. A bare `str` or `(slice a)` written as a field or payload is such a borrow, exactly as `(ref str)` or `(ref (slice a))` is, because Rust cannot store an unsized view by value. The same holds for a view in a type argument of a field or payload, so `:models (vec str)` emits `models: Vec<&'a str>` and `(vec "Leaf" "Ariya")` fills it with no allocation. A function-typed field or payload whose result holds a borrow and whose parameters hold none stores a borrow too, and its result takes the inferred lifetime, because such a function type gives its result nothing to borrow from: `(defstruct factory :make (fn () car))` emits `make: fn() -> Car<'a>`, `:order (fn (bool) car)` emits `order: fn(bool) -> Car<'a>`, and the same holds in a type argument, so `:maker (option (fn () car))` emits `maker: Option<fn() -> Car<'a>>`, and in a function type's result, so `:maker (fn () (fn () car))` emits `maker: fn() -> fn() -> Car<'a>`. A parameter marked unsized in its `rust-type` head keeps its view, so a `(box str)` field keeps `Box<str>` and a `(box (vec str))` field keeps `Box<Vec<&'a str>>`, because the mark reaches the box's own argument and not a view deeper inside it. The source keeps writing the bare type name, and a lifetime written in the head is exact and replaces the inferred one. A function that returns such a value whose borrowed field or payload holds a borrow of a local gets `L0510`, with a help that names the field and the owned type to declare, `string` for each `str` and `(vec a)` for each `(slice a)`, so a `(vec str)` field gets `(vec string)`. Each type parameter of a struct must appear in a field. Each parameter of a Rust type must appear in its template. A type parameter takes no traits, because Rust would demand a type's traits at every use. A function that needs a trait names it in its own `where`. A type takes exactly as many arguments as it has parameters.

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

After inference, a function whose every use outside its own recursive group gives each type parameter the same concrete type emits those types in place of the parameters, as `is_alive(grid: &[Vec<bool>], x: usize, y: usize) -> bool` does. A function used at two or more types stays generic, and so does a function used at a type that holds another function's type parameter, as `(apply thunk)` inside a generic function is. A written `type` signature stays exactly as written.

A type parameter satisfies a trait when its `where` names the trait, or when every type its traits allow implements the trait. `(n float)` therefore satisfies `number` and `display`. The emitted Rust names any such bound that the runtime traits' supertraits do not already give.

### Literals

- An integer literal can become any number type, so `(->point 1 2)` emits `make_point(1.0, 2.0)`.
- A float literal can become `f32` or `f64`.
- Every literal is checked against the range of its type. A float literal too large for its type, such as `1e400` or `(the f32 1e39)`, is an error, and one too small, such as `1e-400`, rounds to zero, as in Rust.
- A call pins a literal parameter the way it pins any inferred parameter, so a helper called once with a `usize` keeps `usize` in its parameter and its literals.
- A use pins a literal in a function's result the same way, so `(defn port () 8080)` passed only where a `u16` is expected emits `fn port() -> u16`.
- A pin passes through other unannotated functions. `(defn other () (port))` used only where a `u16` is expected pins both `other` and `port`, a wrapper `(defn wrap (y) (build y))` takes the `u16` that `build` gives `y`, and `(build-with ->car)` pins the `2025` that `(defn build-with (constructor) (constructor "Nissan" 2025))` passes to its function parameter. Two uses that want different types leave the literal open, and the default then fails at the first use that disagrees.
- A literal whose type is still open after pinning defaults to `i64` or `f64`. The emitted literal carries a suffix (`3_i64`) wherever rustc would otherwise infer a different type. An argument at a type parameter is such a place: `(twice 3000000000)` emits `twice(3000000000_i64)` when `twice` is generic. A branch or a block in that place carries the suffix too. So does a payload, a field or a closure result. A literal keeps no suffix where a written type, another argument or the rustc default already fixes its type. So `(larger 1 odometer)` emits `larger(1, odometer)`.
- A block that binds a literal to a name and returns that name emits the literal alone, with its type's suffix, so `(the u16 0)` emits `0_u16`.
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

### Type texts

`(infer expression)` is a `(ref str)` that holds the expression's type as a `type` signature writes it, through the writer that every diagnostic's type uses ([RFC 0054](../rfcs/0054-infer.md)). The checker checks the expression in place, so it sees the enclosing scope, and its constraints count, bounds included: `(defn wrap (x) (println (infer (str x))) (some x))` generalizes to `(where ((a display)) (fn (a) (option a)))`. The program never runs the expression, and the ownership pass and the emitter never see it, so `(println (infer (vec 1 2)))` emits `println!("{}", "(vec i64)")`, `(infer s)` after `s` moved is allowed, and `(infer (println "x"))` prints nothing and is `"unit"`.

- The text is written after the enclosing definition's inference, pinning and literal defaulting, so `(infer 1)` is `"i64"`, and `(let x 1) (println (infer x)) (the u8 x)` prints `u8`.
- A type variable that appears in no binding outside the expression is generalized with its bounds: `(infer none)` is `"(where (a) (option a))"`, and `(infer identity)` is `"(where (a) (fn (a) a))"`. A variable that only a bound names is generalized too, so the bounds paste into a signature whole: `(infer eq)` is `"(where ((a (partial-eq b)) b) (fn (a b) bool))"`. The generalized variables take the next names that the enclosing definition's type does not use, so `(defn wrap (x) (println (infer (pair x none))) (some x))` prints `(where (b) (pair a (option b)))`. A variable that a binding outside the expression shares is not generalized, and one the definition never fixes is `L0407` at its binding. A variable that stays inside the expression is no error, because the operand never emits: `(infer (len (vec)))` is `"usize"`.
- A call in the expression is a use under the pin rules, as it is outside `infer`, so `(infer (wrap "s"))` beside `(wrap 1)` keeps `wrap` generic. `(infer (port))` beside `(listen (port))` is `L0401`, as `(prn (port))` is there.
- A parameter of a generic function gives its type variable, so `(infer x)` in `wrap` is `"a"` on every call, also when `wrap` emits with one concrete type.
- A function type is written `fn`, never `fn-mut` or `fn-once`, and a parameter's passing mode does not show: `(defn total (v) (println (infer v)) (reduce (iter v) 0 (fn (sum n) (+ sum (deref n)))))` prints `(vec i64)`, though `total` takes `&[i64]`.
- A text pastes into a `type` signature where the parameters' modes keep their defaults. Ownership infers a mode after the checker, so a text leaves it out, and a result that borrows its parameter does not check when pasted: `(infer (fn (x) (iter x)))` is `"(where (a) (fn ((vec a)) (iterator (ref a))))"`, and that text on `(defn elements (x) (iter x))` is `L0510`, where `types` prints `(ref (slice a))` for the parameter.
- Two types of one name from two modules print alike, as in diagnostics.
- `infer` needs the checker, so macro code, which runs before it, cannot use the form: `infer` at compile time reports L0316, as `try` does.

### Standard types

The prelude binds these Rust types, the traits that abstract them, and the functions that use them:

| Type | Rust | Functions |
|---|---|---|
| `(vec a)` | `Vec<A>` | `vec push! len iter copied collect get pop! remove-at! sort! sort-by! windows at dedup! first last empty? reverse reverse!` |
| `(slice a)` | `[A]` | `iter windows at first last empty? reverse` |
| `(box a)` | `Box<A>` | `box` |
| `(pair a b)` | `(A, B)` | `pair pair/pair let-pair` |
| `(option a)` | `Option<A>` | `some none option/some option/none some? none? unwrap-or unwrap expect map-some then-some` |
| `(result a b)` | `Result<A, B>` | `ok err result/ok result/err ok-or ok? err? unwrap-ok unwrap-err expect-ok map-ok map-err then-ok` |
| `(iterator a)` | `impl Iterator<Item = A>` | `range map filter reduce for-each enumerate zip sum count max min max-by-key any? every? find take take-while` |
| `char` | `char` | `alphanumeric? whitespace? digit?` |
| `str`, `string` | `&str`, `String` | `display str char print println eprintln chars lines split trim subs as-str parse parse-error from-str lower-case upper-case split-when push! empty?` |
| `(sorted-map key value)` | `std::collections::BTreeMap<K, V>` | `sorted-map get keys vals iter update into-sorted-map assoc set! len empty?` |
| `(hash-map key value)` | `std::collections::HashMap<K, V>` | `hash-map get keys vals iter update into-hash-map assoc set! len empty?` |

`print`, `println` and `eprintln` take any number of parts. Each string literal part folds into one format string, and each other part becomes a `{}` placeholder. `eprintln` writes to standard error. `str` folds its literal parts the same way, and with one part that is not a string literal it emits `x.to_string()`, so `(str year)` emits `year.to_string()`. `str` as a value is that one-part function, so `(map (iter years) str)` turns each year into a `string`. A `print` whose last part ends in a newline emits `println!` without that newline. `prn` and `pr-str` print a value as a form the reader reads, through the `show` trait, described under Printed forms. `(display type (value) parts...)` implements `display` for a struct or enum the program defines, and makes the type printable with `print`, `println` and `str`. `subs` takes a `str`, a start and an end, and returns the `str` between them, so `(subs line 2 5)` emits `&line[2..5]`. `pi` is Rust's `std::f64::consts::PI`, an `f64`. `remove-at!` removes the element at an index and returns it, so `(remove-at! items 0)` emits `items.remove(0)`.

### Printed forms

`unwrap-ok`, `unwrap-err` and `expect-ok` panic with the printed form of the other side, as in `called unwrap-ok on (err (->failure :line 3))`, so they need `show` of it. `option`'s `unwrap` and `expect` need no bound.

`prn` prints a value's printed form and a newline, `pr-str` returns it as a `string`, and a `(pr-str x)` part of `print`, `println`, `eprintln`, `str` or `display` prints it in place. All three need the `show` trait, backed by `Show` in the runtime. A printed form uses only atoms and lists, and where the source can build the value, it is the source that builds an equal value, so a printed result pastes back into a program:

| Value | Prints as |
|---|---|
| an integer, such as `(the u8 7)` | `7`, with no type suffix: the type comes from the place, as a literal's does |
| a float, such as `(the f32 0.1)` or `1e100` | `0.1`, `1e100`: Rust's shortest round-trip text, which always holds a `.` or an `e` |
| infinity, negative infinity, NaN | `(/ 1.0 0.0)`, `(/ -1.0 0.0)`, `(/ 0.0 0.0)` |
| `true` | `true` |
| `(char "a")` | `(char "a")` |
| a `(ref str)` or a `string` | `"hello \"ana\"\n"`, with only the escapes the reader knows |
| `unit` | `(do)` |
| `(some 3)`, `none` | `(some 3)`, `none` |
| `(ok 1)`, `(err (str "bad"))` | `(ok 1)`, `(err "bad")` |
| `(pair 1 "a")` | `(pair 1 "a")` |
| a vector or a slice | `(vec 1 2 3)`, or `(vec)` when empty |
| `(ref x)` | what `x` prints |
| `(box 5)` | `(box 5)` |
| a `sorted-map` | `(sorted-map "a" 1 "b" 2)`, in key order |
| a `hash-map` | `(hash-map "amy" 2 "zed" 1)`, sorted by each key's printed text, so one program prints the same text on every run |
| `(->point 4 3)` | `(->point :x 4 :y 3)` |
| `(shape/circle 1.0)`, `shape/empty` | `(shape/circle 1.0)`, `shape/empty` |
| a variant with a struct payload | `(shape/labeled "box" 2.0)`, its fields by position |
| `io-error`, a `parse-error` | `(io-error "No such file or directory (os error 2)")`: the type applied to its `display` text, which reads but does not evaluate |

Nested values nest their forms, and a printed form never holds a newline. A struct and an enum print through `show`'s `structural` clause in `prelude/text.rslisp`, so a struct prints as the `->` call that `defstruct` defines, and a kernel `struct` prints a form that does not evaluate. The `Box` that a recursive enum requires prints as nothing, because the source never writes it. A closure, an iterator and a function have no source that rebuilds them, so they do not implement `show`, and neither does a struct or enum whose field or payload holds a function type, so no printed text holds an address that changes between runs.

A program writes its own form with `(show type (value) form)`, a prelude macro over `impl`, as `display` is. The form is a list whose head is a symbol, and each argument prints with `show`, so `(show money (m) (dollars (/ (:cents m) 100)))` prints `(dollars 12)` for 1200 cents. A bare symbol prints its name, so `(show origin (o) origin)` prints `origin`. Any other form is `L0301`.

`len` takes any `sequence`, a trait backed by `Sequence` in the runtime and implemented for `String`, `str`, `(vec a)`, `(slice a)`, `(sorted-map k v)`, `(hash-map k v)` and a reference to any of them. `empty?` tests the same trait. `sort!`, `sort-by!`, `max`, `min` and `max-by-key` take the `ord` trait, which lists the same implementors as `partial-ord`, except `f32` and `f64`. A two-argument `sort!` takes an order: a written `<` sorts ascending, as `(sort! v)` does, and a written `>` sorts descending, so `(sort! v >)` emits `v.sort_by(|left, right| right.cmp(left))`. Any other second argument is a comparator of type `(fn ((ref a) (ref a)) i64)`, which sorts as in Clojure: a negative result puts the left value first. `parse` takes a `from-str` target and returns `(result target (parse-error target))`, and emits `text.parse::<target>()`. `parse-error` is the associated type `Err` of `from-str`, so `(parse-error i64)` is `parse-int-error` and emits `std::num::ParseIntError`, and `(parse-error target)` on a type parameter emits `<Target as std::str::FromStr>::Err`. The prelude exports each value of `parse-error`: `parse-bool-error`, `parse-char-error`, `infallible`, `parse-int-error` and `parse-float-error`. Every `parse-error` a target names implements `display`, `debug` and `show`, so `(str reason)` and `(prn reason)` print it.

An iterator is lazy, as in Rust, so `(reduce (range 1 1000001) 0 (fn (total n) (+ total n)))` repeats work without recursion. `reduce` with an initial value emits `.fold(initial, step)`, and without one it emits `.reduce(step)`, which returns an `option` that is `none` for an empty iterator. `(vec 1 2 3)` emits `vec![1, 2, 3]`, and `(vec)` emits `Vec::new()`.

`sorted-map` builds a `BTreeMap` from key and value pairs, and `(sorted-map)` is empty. Its keys implement `ord`, and its entries iterate in key order. `hash-map` builds a `HashMap` the same way; its keys implement `hash` and `eq`, and its iteration order is not fixed. `into-sorted-map` and `into-hash-map` collect a vector of pairs. `get` returns `(option (ref value))`, and a `str` may query a `string` key. `keys` and `vals` return iterators of references, and `iter` returns an iterator of `(pair (ref key) (ref value))`. `update` removes the value or uses the default, applies its function, inserts the result and returns the map, so no value is copied. `assoc` inserts a key and returns the map. `set!` on `(at m k)` borrows the key and emits `*m.get_mut(&k).unwrap() = v`; it changes an existing key and panics on a missing one. `len` and `empty?` take a map through `sequence`. The `map-at`, `assoc-at` and `set-at` collection rules pick all of this from the inferred map type, so the prelude has one API for every map. A key that lacks its map's key traits is the existing missing-trait error.

Program input and output use Rust's standard library through the prelude. `stdin` names both a type, Rust's `std::io::Stdin`, and the program's standard input as a value. `slurp` reads all of a file or of standard input and returns `(result string io-error)`, so `(slurp "cars.txt")`, `(slurp path)` and `(slurp stdin)` all work. It takes the `source` trait, which `str`, `string` and `stdin` implement, by reference, so an owned `string` path stays usable after the read, and an unannotated `(defn load (path) (slurp path))` infers `(ref str)`. `(slurp "cars.txt")` emits `std::fs::read_to_string("cars.txt")`, an owned `path` passes as `&path`, and `(slurp stdin)` emits `std::io::Write::flush(&mut std::io::stdout()).and_then(|()| std::io::read_to_string(&std::io::stdin()))`, through the clauses of `slurp`. Every form that reads standard input flushes standard output first, so a prompt that `print` writes without a newline shows before the read waits. A function generic over `source` emits `crate::rt::Source::slurp(source)`, which brings the runtime's `Source` part into the program. `(line-seq stdin)` streams standard input one line at a time as an `(iterator (result string io-error))`, so a filter holds one line in memory, and emits `crate::rt::lines(&std::io::stdin())`, which brings the runtime's `Lines` part into the program. The stream locks standard input for one line at a time, so `read-line` and `(slurp stdin)` may read between its lines. Each line drops a trailing newline and carriage return, as `read-line` does. A failed read, including a line that is not valid UTF-8, yields one `err` and then ends the stream, so the program sees the failure and a read that keeps failing cannot loop forever. `line-seq` reads no line after a line that is not valid UTF-8, and a later `read-line` reads the next one. `lines` splits a string only. `read-line` returns `(result (option string) io-error)`, returns `none` at end of input, and drops a trailing newline and carriage return. `io-error` is Rust's `std::io::Error`, which implements `display`, `debug` and `show`. `command-line-args` returns the arguments after the program name. `exit` stops the process with an `i32` code.

A `main` whose inferred type is a `result` emits an inner function for its body and a `fn main()` that prints its error with `display` to standard error and exits 1. The success path exits 0. The error type must implement `display`; a program's own enum implements it with `display`. A `main` whose inferred type is neither `unit` nor a `result` is an error, because Rust requires `main` to return a type that implements `Termination`.

- `iter` yields `(ref a)` for a vector or a slice, and `(pair (ref key) (ref value))` for a map. `copied` yields copies, so its elements must be `copy`.
- `(count (iter coll))` emits `coll.len()`, because the collection already knows its length. `count` is a prelude macro that rewrites only that written form, so `count` on any other iterator, and on a vector that the compiler iterates for it, emits `.count()`. `count` as a value, as in `(let counter count)`, emits the path `Iterator::count`, because rustc cannot infer the parameter type of a closure that calls a method.
- `filter` passes each element to its test as a reference, as Rust does, and so do `find`, `take-while` and `max-by-key`. Over `(iter years)` the test's parameter is `(ref (ref i64))`, so `(filter (iter years) (fn (y) (> (deref y) 2020)))` emits `years.iter().filter(|y: &&i64| **y > 2020)`: the comparison copies the year out of the borrow that `deref` leaves, as the Adjustment bullet says.
- `range` takes an `integer`, a prelude trait of the integer types.
- An `iterator` is an error, `L0458`, anywhere in a struct field or an enum payload, and inside a function type in a top-level function's parameter or result or in a trait method's signature, because Rust names an iterator's type only as the type of a function's own parameter or result. `(defstruct feed :items (iterator i64))` would emit `items: _`, `(defstruct tally :counter (fn ((iterator (ref i64))) usize))` would emit `counter: fn(_) -> usize`, and passing `count` to `(defn tally (counter years) (counter (iter years)))` would emit `counter: impl FnOnce(_) -> usize`. Store or pass the collection and call `iter` where the items are used instead. `L0458` does not stop the ownership checks, so a program whose only type errors are `L0458` also reports its ownership errors.
- A closure bound by `let` whose parameter holds an `iterator` and whose body calls a method on that parameter is an error, `L0458`, because Rust must know a closure parameter's type before the closure calls a method on it. `(let tally (fn (it) (count it)))` would emit `|it: _| it.count()`. A closure that passes the parameter to a function, as `(fn (it) (sum-of it))` does, or returns it, leaves the type for rustc to infer.
- A function generic over `integer` can call `range`: the prelude's `step` trait states Rust's requirement `Range<N>: Iterator<Item = N>`, and the function's emitted signature carries it as a `where` predicate.

## Structs

Programs write structs with the prelude's `defstruct`, which takes flat fields. `(defstruct point :x f64 :y f64)` declares each field as a keyword followed by one type form, so `:tags (vec str)` is one field. It expands to the kernel `struct` form and defines one constructor, `->point`. A field keyword whose name ends with `:` is `L0332`. `(defstruct user :name str :email: str)` gets help that writes `:email`, and a keyword made only of `:` characters, such as `::`, gets help that asks for a name.

The constructor `->point` is a macro, and it reads a call in this order:

1. If the arguments are `:field value` pairs that name distinct fields and every field without a default, the call is by name. `(->point :y 2.0 :x 1.0)` expands to `(new point (:y 2.0) (:x 1.0))`, in the order written.
2. Otherwise, if the arguments are `:key value` pairs, the call is positional only when it has one argument per field and every keyword at an even position lands on a field whose written type is a function type, `fn`, `fn-mut` or `fn-once`. With `(defstruct sorter :key (fn ((ref user)) (ref str)) :label str)`, `(->sorter :name "by name")` is positional. A type parameter does not count as a function type.
3. Otherwise, `:key value` pairs are a call by name. The call expands to `new` with the defaults of the omitted fields, and `new` reports an unknown key in the user's spelling with the closest field, as `(->car :mak "Nissan")` suggests `:make`, a repeated key, or the missing fields that have no default.
4. Any other call with exactly one argument per field is positional. `(->point 1.0 2.0)` calls a positional function that `defstruct` defines under a hygienic name, which no source code can refer to. It emits as `make_point(1.0, 2.0)`. Any other call is an error that names the field count.

In value position, `->point` expands to the positional function, so `(let build ->point)` emits `let build = make_point;`. A program may define its own `make-point`, because no source name refers to the positional function.

A `defstruct` may end with one `(defaults :field value ...)` clause, recognized by its `defaults` head, in a place where a field keyword would go. Each key names a declared field at most once. `(defstruct user :name str :email str :admin? bool (defaults :admin? false))` lets a call by name omit `:admin?`, and `(->user :name "ana" :email "ana@example.org")` expands to `(new user (:name "ana") (:email "ana@example.org") (admin? false))`, which emits `User { name: "ana", email: "ana@example.org", is_admin: false }`. The default is inlined at each construction that omits it, so it takes its field's type there, and a default such as `(vec)` builds a new value at each construction. A default cannot refer to other fields. A positional call and `->user` in value position still take every field.

### Field keywords

A keyword reads a field. A list whose head is a keyword and that has one argument expands to the kernel `field` form: `(:x origin)` is `(field origin x)`, and it borrows, moves and reports an unknown field exactly as that form does. A read through a written `deref` emits no `*`, because a Rust field access dereferences by itself, so `(:year (deref c))` emits `c.year`. A keyword in value position expands to a closure that reads the field, so `(map (iter points) :x)` emits `points.iter().map(|value: &Point| value.x)`. Each such closure is typed where it is used.

A keyword call with no value or with more than one value is an error. `let`, `def`, `fn` parameters and patterns reject a keyword as a name. The rule runs on the expanded program, so a macro receives a keyword unchanged, as `defstruct` does.

A field label in `new` may be written as a keyword, so a macro passes the user's spelling through, as `assoc` and `->car` do. An unknown field then reports and suggests the field as a keyword, as `(assoc leaf :yer 2025)` suggests `:year`.

A field read names its struct only through the value's type. When two structs share a field name, a function that reads that field from an unannotated parameter needs a signature, as `(type volume (fn ((ref cube)) i64))` gives.

The expander carries keyword reads because a prelude macro per field would clash between two structs that share a field name.

## Ownership

Ownership is part of the type, as in Rust. `(ref t)` is `&T` and `(mut t)` is `&mut T`. Type inference ignores ownership except where it is written, and a separate pass decides how each parameter is passed.

- **Modes.** A parameter without a written type gets the cheapest mode its uses need: copy for a Copy type of 16 bytes or less, then ref, then mut, then own. A type whose size the compiler cannot compute, such as a type parameter, counts as 16 bytes or less. A use through a call follows the called function's mode, and the modes are recomputed until nothing changes.
- **Copy types.** A Copy type implements the prelude's `copy` trait, whose Rust path is `Copy`. A type parameter is Copy when its `where` names `copy`. The parameter is also Copy when one of its traits allows only Copy types. When a body copies a generic parameter that no written signature declares, the compiler infers the parameter's `copy` bound. A written signature is exact: if it declares the parameter without `copy`, the second use is an error that suggests `(a copy)`.
- **Written types.** A type written without `ref` or `mut`, in a `type` signature or a binder annotation, means owned, as in Rust.
- **Adjustment.** Wherever a value meets an expected type, the compiler adds one borrow, removes extra borrows, copies a Copy value of 16 bytes or less out of a borrow, or shrinks a borrow's referent from a vector to its slice, or a shared borrow's referent from `string` to `str`. A vector or map where the expected type is `(iterator a)` is iterated: the compiler writes `.iter()`, yielding `(ref a)` for a vector and `(pair (ref key) (ref value))` for a map. A vector yields copies instead, by `.iter().copied()`, when the expected item is a Copy type that the vector's elements are, or a type parameter whose traits allow only Copy values, such as `number`. Elements from number literals are any number type their literals allow. `(sum scores)` emits `sum(scores.iter().copied())`, and with a `(iterator (ref str))` parameter, `(count-long names)` on a `(vec (ref str))` emits `count_long(names.iter().copied())`. It never clones. The compiler writes each adjustment in the emitted Rust, except the slice and `str` ones: Rust's deref coercion already makes `&Vec<T>` meet `&[T]`, `&mut Vec<T>` meet `&mut [T]` and `&String` meet `&str`, so no syntax changes. A closure's result is not a coercion site in Rust, so a closure whose `(ref string)` result meets `(ref str)` calls the prelude's `as-str`: with a `(fn ((iterator (ref user))) (iterator (ref str)))` signature, `(map users :name)` emits `users.map(|value: &User| value.name.as_str())`. The first of several arguments that share a type variable is not a coercion site either, because Rust fixes the variable from it, so a written `ref` of a `string` that shares a type variable with a `(ref str)` argument calls `as-str` too, and the compiler views the borrow at every position so the order of the arguments never changes the emitted Rust: `(> (ref model) "Civic")` and `(> "Civic" (ref model))` emit `model.as_str() > "Civic"` and `"Civic" > model.as_str()`. The slice adjustment lets a vector parameter that no use needs as a vector take a slice, and the `str` adjustment lets a read-only `string` parameter take `&str`. `(add x 2)` with `x : (ref i64)` passes where Rust's generic `add(x, 2)` fails, because the compiler writes the `*`. The Call sites and Dereferences bullets give the details. A written `ref` or `mut` is not adjusted: it meets a parameter with a written type as written, and a parameter with no written type as the mode its body infers. A written `deref` removes exactly one borrow. A result that is still a borrow meets its expected type by the rule of [8.1](#81-references-at-call-sites): the compiler adds one borrow, removes borrows, or copies a Copy value out, and does not iterate it. So with `y : (ref (ref i64))`, `(> (deref y) 2020)` emits `**y > 2020`, as `(> y 2020)` does, and `(let (year i64) (deref y))` emits `let year: i64 = **y;`. Any other result of a written `deref` is not adjusted.
- **Call sites.** An argument is borrowed automatically when the parameter is a reference. A method-style template such as `"{0}.clone()"` borrows its receiver the way Rust's method calls do. A receiver or a `{&0}` argument is written without `&`, even when it reads a field, because Rust borrows it. A `{&0}` argument stays usable after the call: `(if (> a b) a b)` moves neither `a` nor `b` until it returns one of them. A receiver that names a static is written without `*`, because a method call dereferences it.
- **Dereferences.** A borrowed Copy argument of 16 bytes or less is dereferenced when the parameter's type is a type variable whose traits the reference lacks. `(> x 2)` with `x : (ref i64)` emits `*x > 2`. A `fn` passed as an argument takes its parameter types from the parameter's function type, so its body sees them. A `fn` written as a field value in `new` takes them from the field's function type the same way, so `(new sorter (:key :name) (:label "by name"))` reads `name` through the field's `(ref user)` parameter. An argument with more reference layers than its reference parameter passes with the extra layers removed, as Rust's deref coercion does, even when the parameter's referent is still unknown. Rust's coercion stops at a type parameter, so where the parameter is a reference to a type parameter of a called `defn`, the compiler writes the dereference: with `(type apply-to (where (a) (fn ((ref a) (fn ((ref a)) bool)) bool)))`, `(apply-to view (fn (text) (= text "a")))` on a `(ref (ref str))` emits `apply_to(*view, |text: &str| text == "a")`. A trait method without a receiver follows the same rule at a reference to `self` or to an associated type, because Rust picks the implementor from the argument as written: with `(matches (fn (i64 (ref self)) bool))`, `(matches 1 outer)` on a `(ref (ref sized-thing))` emits `Matcher::matches(1, *outer)`. A receiver fixes the implementor, so a method with a receiver leaves its arguments to the coercion, and a method call dereferences the receiver itself: `(area outer)` emits `outer.area()`. Elsewhere the extra layers are left to the coercion, so `(total outer)` on a `(ref i64)` parameter emits `total(outer)`. A `rust` template takes the argument as written. A type parameter never takes a `str` or a slice where the signature holds it outside a reference, because Rust stores only sized values: with `(type empty-like (where (a) (fn ((ref a)) (vec a))))`, `(empty-like view)` on a `(ref (ref str))` takes `(ref str)` for `a` and emits `empty_like(view)`. If an argument's referent type is still unknown at a call, the compiler decides its borrow after it infers the definition. A borrowed non-Copy argument passes as is when the parameter's type is a type variable whose traits the referent has and the reference lacks: `(size-of (as-str word))` emits `size_of(word.as_str())`.
- **Shared parameters.** When one type variable is the whole type of several parameters, as in `(= a b)`, the compiler decides their borrows together after it infers every argument, so the order of the arguments never changes the result. Two type variables linked by a two-type bound form one group. The compiler first tries the arguments exactly as written: if the bound holds with each variable bound to its argument's type, every argument passes with no borrow. Otherwise it applies the shared rule. A value whose type is still an open literal type, such as `2020` or a local bound to it, beside a written `ref` of a value, takes one borrow wherever it stands, unless a Rust template borrows the literal's position or the written `ref`'s position: with `(type later? (where ((a partial-ord)) (fn (a a) bool)))`, both `(later? (ref year) 2020)` and `(later? 2020 (ref year))` pass `&2020`. A Copy value of 16 bytes or less is copied out of every borrow. Arguments with the same number of references pass as they are. Otherwise, each argument gets one reference, or none where Rust borrows the argument by itself: `(= (:name p) name)` emits `p.name == name`. `partial-eq`, `partial-ord` and `display` hold for a reference to any type that has them, as in Rust: `(< (:name ana) (:name bo))` emits `ana.name < bo.name`.
- **Fields.** Reading a Copy field of 16 bytes or less copies it. Reading any other field borrows it: `(field person name)` has type `(ref string)`. The base of a collection update that reads a field of an owned local, at any depth, moves that field instead. A struct update base moves the base's fields that the update does not list and that are not Copy, as Rust does, and `(assoc (:start line) :x 3)` emits `Point { x: 3, ..line.start }`. A later use of the local, or of a moved field or a field inside it, is a use-after-move error. A struct update reads only the base's fields that it does not list, so `assoc-in` may update a field whose inside it moved, and the base's moved fields stay moved. A read of a field that did not move stays allowed, as in Rust. A field declared with a function type holds a `fn` pointer, which Rust copies, so reading it copies it, even through a reference, and `((:key chosen) someone)` emits `(chosen.key)(someone)`. A function type elsewhere is not Copy, because its value may be a closure that owns its captures.
- **Unsized values.** Rust never owns a `str`. A `str` parameter is always borrowed, a parameter of a closure bound by `let` included, so `(let build (fn (make) (->car make 2025)))` takes `make` as `&str`, and a function whose result is a `str` returns `(ref str)`.
- **No hidden copies.** Copying heap data is always written `(clone x)`, which needs the `clone` trait. The one inferred allocation is the `Box` a recursive enum payload requires. A struct implements it when its fields do, as the Struct traits bullet says. `(ref x)` and `(mut x)` force a borrow.
- **Struct traits.** A struct implements `clone`, `partial-eq`, `eq` and `hash` when all its field types do, and the emitted struct derives them. The compiler does not infer `debug`, because nothing in the language prints with Rust's `Debug`; a written `derive` still lists it for interop. It implements `show` when all its field types do, through `show`'s `structural` clause. It implements `copy` too when all its field types are Copy and the program never changes it in place. A function type in a field type, directly or inside another type such as `(option (fn () car))`, allows `clone` and `copy`, and a written `debug`, because it emits as a `fn` pointer, and blocks comparison, hashing and `show`, because Rust compares function pointers unreliably and a function has no printed form. The rest of the field type still needs each trait, so `(pair (fn () i64) badge)` derives `clone` only when `badge` does. The compiler fixes these Rust derive names, and it also infers every trait whose `rust-trait` holds a `structural` clause, for every struct and enum whose field types implement it, with each type parameter bounded by the trait. A function type blocks such a trait. Only the compiler can infer it, because only the compiler knows a struct's fields and whether each field's type implements the trait. It emits the impl from the clause's templates, and only for a type whose impl a reached use needs, directly or through a field or an implementor's bound, so a program that never uses the trait on a type emits no impl for it. A written `impl` of such a trait replaces the inferred impl for every instance of the type's head and leaves its other traits inferred. A written `(derive point clone debug)` lists a struct's traits exactly and turns inference off for it. It may list a trait with a `structural` clause, and then emits that trait's impl from the template rather than a `#[derive]` name. `L0422`'s help lists those traits, read from the prelude's and the program's bindings, after the derivable Rust traits. A listed trait brings the traits Rust requires of it: `copy` brings `clone`, `eq` and `partial-ord` bring `partial-eq`, and `ord` brings `eq` and `partial-ord`. `derive` is a kernel form, because the prelude cannot add attributes to an emitted struct. The program changes a struct in place when a `mut` borrow takes a value of the struct or a place that reads one of its fields, or when a written type is `(mut s)`.
- **Moves.** The compiler reports a use after move and points at the move. A top-level `def` whose value is not a `fn`, a `rust` form, or a use of a `def` that holds no value holds one value for the whole program, so a use can only borrow it: a use that needs to own a non-Copy one, such as a field value, an owned argument or another `def`'s value, is an error, `L0503`, whose help suggests `(clone x)`. A borrow such as `(len x)` stays allowed, and a Copy value is copied. A `match` whose arms bind no non-Copy part borrows the def, so `(match maybe-name ((some _) "named") (none "none"))` stays allowed. The error names the moved value as the source writes it: a local by its name, and a moved field by its read, as `(:badge g)`. It suggests `(clone x)` only when the clone keeps the meaning: the value must not change in place after the move, must not be a `mut` borrow, and must not be passed to a recursive call of the enclosing function. Otherwise the help names the real fix. A moved `iterator` gets no clone suggestion, because a cloned pipeline repeats its work: the help says to collect it, as in `(collect names)`, or to write a function that rebuilds it, and a borrowed `iterator` that a call needs to own gets the same note and help, with `this value` in place of its name. A struct update whose base is a name or a field read evaluates its field values before its base, as Rust does, so a field value that reads the base sees it unmoved, as `(assoc-in g (:badge :size) (+ 1 (:size (:badge g))))` does. Any other base binds with `let` and runs before the field values. A move in either branch of an `if` counts. A body that `recur` repeats runs again after each `recur`, as a Rust `loop` does, so a local bound outside the body that the body moves on a path that reaches `recur` is moved when the next pass uses it. A move on a path that returns, and a move of a local the body binds, end with the pass.
- **Function values.** A named function passed where a function type is expected is wrapped in a closure when its parameters need an adjustment. The compiler decides the adjustments after ownership inference, so a parameter whose inferred mode adds a borrow also wraps the function value. An expected parameter that is a reference and a function parameter that is not is the same rule before modes. The closure passes each argument through the usual adjustments, and a Copy value copies out through every borrow: with `(defn recent? (y) (> y 2020))`, `(filter (iter years) recent?)` emits `years.iter().filter(|argument: &&i64| is_recent(**argument))`. A function parameter that the closure copies into takes the type the argument holds behind its borrows, before a literal in the body defaults it, as a closure's parameter takes its expected type: with `(defn small? (b) (< b 10))`, `(take-while (iter bytes) small?)` over a `(vec u8)` gives `small?` the type `(fn (u8) bool)` and emits `b < 10`. A function also called directly with a literal follows the Literals rule for two uses, so `(small? 5)` beside that use is an error, `L0401`, at the use that disagrees, whose note names the value use. Whenever a value use takes part in the disagreement, the help suggests the function's inferred signature with each type parameter declared, the bounds its body needs, and, for a literal's parameter, the trait whose implementors are the literal's types, `(type small? (where ((a partial-ord number)) (fn (a) bool)))`, so that each use takes its own type. A parameter that ownership narrows to a view takes its `vec` or `string` item through Rust's deref coercion, so with `(defn width (row) (count (iter row)))`, `(map (iter grid) width)` over a `(vec (vec i64))` emits `fn width(row: &[i64])` and `|argument: &Vec<i64>| width(argument)`. A generic view such as `(slice a)` takes the vector's element type for `a`. Both function types first take the types the call gives their type parameters, so an unannotated `(defn apply-one (f a) (f a))` called as `(apply-one increment 41)` expects `(fn (i64) i64)`. If no adjustment makes a parameter match, the compiler reports a mismatched function type and names the parameter.
- **Reborrows.** A `mut` reference passed to a `mut` parameter is reborrowed and stays usable. Passed to a `ref` parameter, it is reborrowed as a shared reference, as Rust coerces `&mut T` to `&T`. Passed to any other parameter, it moves.
- **Mut borrows.** A `mut` argument must be a place: a local or a field. A field is borrowed in place, even when the field is not Copy. A copy, such as a call's result, is an error, because changing the copy changes nothing.
- **Assignment.** `(set! place value)` changes a local or a field in place. Inside the place, `(at items index)` names an element, so `(set! (at (at grid y) x) alive)` emits `grid[y][x] = alive`. A value-position `(at items index)` returns a borrow of the element, so a non-Copy row reads without `get` and `unwrap`. A Copy element meeting a value parameter is copied out, so `(at (at grid 1) 0)` emits `grid[1][0]` there and `&grid[1][0]` where it stays borrowed. An `at` place reads through a `mut` reference, so `(set! (at cars 0) car)` on a `(mut (vec car))` parameter emits `cars[0] = car`, and a `(mut (slice car))` parameter assigns the same way. An unannotated `defn` parameter used as the base of an `at` place with one index takes the collection its callers pass, so `(defn rename! (models key model) (set! (at models key) model))` called with a `(mut (hash-map str str))` emits `*models.get_mut(key).unwrap() = model`. A body that uses the parameter as a vector elsewhere, or a place with more than one index, infers a mutable vector borrow. Callers that all pass vectors or slices make the base a vector, whatever their element types. A borrowed element passed where the same open element type is expected, as in `(set! (at years 1) (at years 0))` or `(push! v (at v 0))`, is copied out and requires `copy`: a function whose callers pass strings reports `L0404` at the call, with a label at each read that copies and a help that suggests `(clone ...)`. A function whose callers pass both a vector and a map, or two map types, is an error, `L0457`, whose help asks for a `type` signature. A vector element assigns through the vector's slice, so such a parameter emits `&mut [T]` unless another use needs the vector, as 8.1 says. A field vector is a place too, so `(set! (at (:items order) 0) 9)` emits `order.items[0] = 9`. A map element assigns through `get_mut`, so `(set! (at m k) v)` borrows `k`, emits `*m.get_mut(&k).unwrap() = v` and panics on a missing key. A target that is not a place is the same error as a `mut` argument. `set!` is prelude code: a local or field target assigns through a `mut` parameter, and an `at` target becomes a vector or map assignment.
- **Value updates.** A keyword key updates a field, and every other key updates a collection. `(assoc value :field value...)` returns a changed struct with Rust's struct update syntax, as `(assoc cell :alive true)` emits `Cell { alive: true, ..cell }`. `(assoc coll key value...)` consumes the collection and returns it changed, as `(assoc grid 1 alive)` emits `{ let mut grid = grid; grid[1] = alive; grid }`, and each further key and value pair updates the result of the one before. `(assoc-in value (step...) item)` walks a path of field keywords and indexes. A keyword step binds the changed field with `let` and updates the struct around it, so `(assoc-in line (:start :x) 3)` emits `{ let start = Point { x: 3, ..line.start }; Line { start, ..line } }`. A run of index steps updates a collection, as `(assoc-in grid (1 0) 7)` emits `{ let mut grid = grid; grid[1][0] = 7; grid }`. A base that is not a name binds with `let` first, so it runs once, and a field-read base updates in place, as `(assoc-in (:badge g) (:at :x) 4)` emits `{ let at = Point { x: 4, ..g.badge.at }; Badge { at, ..g.badge } }`. A chain of field reads rooted in a form that is not a name binds that form with `let` and reads the fields from it, so `(assoc-in (:badge (build)) (:at :x) 4)` emits `{ let base = build(); let at = Point { x: 4, ..base.badge.at }; Badge { at, ..base.badge } }`. A keyword may not follow an index step yet: it reports an error, because Rust cannot move an element out of a vector to update it. A collection update whose last key has no value is an error, and when its keys look like `(field value)` groups, the help shows the keyword form. A struct passed as a collection gets the type error for a base that is not a collection, with help that points to keywords. The base moves, so a later use is a use-after-move error whose help suggests `(clone base)`. `assoc` and `assoc-in` are prelude macros. The `assoc-at` collection rule picks the emitted Rust from the base's inferred type; it knows `(vec a)`, `(sorted-map k v)` and `(hash-map k v)`, and a map update emits `{ let mut m = m; m.insert(k, v); m }`.
- **Call order.** A call that mutably borrows a variable evaluates its other arguments first. A borrow has no side effect, so the observable order stays the same.
- **Borrowed results.** A function may return a borrow. When its parameters contribute more than one lifetime, the compiler adds one shared lifetime, `'a`, to the reference parameters whose borrows can reach the result and to the result, so the result may borrow from any of them. Every other reference parameter stays elided. With one, Rust's elision decides. A function-typed parameter whose call can return the result takes the shared lifetime in every reference position of its type, because Rust's elision does not reach through a function type: `(fn ((fn ((ref str) u16) car)) car)` emits `fn build_with<'a>(constructor: impl FnOnce(&'a str, u16) -> Car<'a>) -> Car<'a>`. A call through a `let` alias of the parameter is a call of it. A parameter passed to a function whose matching parameter takes the shared lifetime takes it too, so `(defn build-through (constructor) (build-with constructor))` emits `'a` like `build-with`. It stays elided when a call of it in the body passes a borrow of a local, because that borrow cannot live as long as `'a`. An argument outlives the body when it is a literal, a global, a parameter, a local bound to one of those, a call whose arguments all outlive the body, or an `if`, `match` or block whose every branch outlives the body. Every other argument that holds a borrow counts as a borrow of a local. A function type has no body to trace, so under the same condition it binds one higher-ranked lifetime over every elided borrow of its parameters and its result: with `(defstruct user :name str)`, the field type `(fn ((ref user)) (ref str))` emits `for<'b> fn(&'b User<'b>) -> &'b str`, which accepts both a field-read keyword and a named function such as `fn name_of<'a>(someone: &'a User<'a>) -> &'a str`. The binder takes the first name that the enclosing item does not declare. A function type whose result holds a borrow and whose parameters hold none holds a borrow wherever it appears, so a parameter of that type that the body stores into the result shares the result's lifetime: `(type plant (fn ((fn () car)) factory))` emits `fn plant<'a>(make: fn() -> Car<'a>) -> Factory<'a>`. A function whose parameters hold no borrow gives each elided borrow of its result `'static`, because its result has no parameter to borrow from: with `(struct snippet (text (ref str)))`, `(defn literal-snippet () (new snippet (text "hi")))` emits `fn literal_snippet() -> Snippet<'static>`. Such a function returns a function type whose result holds a borrow with `'static` in that result too: `(type leaf-maker (fn () (fn () car)))` emits `fn leaf_maker() -> impl Fn() -> Car<'static>`. An `iterator` parameter whose item holds an elided borrow takes a named lifetime of its own in every elided position of the item, because Rust rejects an elided lifetime inside `impl Iterator`: `(fn ((iterator (ref str))) usize)` emits `fn count_long<'a>(names: impl Iterator<Item = &'a str>) -> usize`. One whose items can reach the result takes the shared lifetime instead: `(fn ((iterator (ref str))) (option (ref str)))` emits `fn last_name<'a>(names: impl Iterator<Item = &'a str>) -> Option<&'a str>`. A function-typed parameter whose result holds a borrow and that no other rule names takes a named lifetime of its own in every elided position of its type, because Rust's elision would accept only a closure that works for every lifetime, and a type with no borrowed argument leaves the result nothing to borrow from: `(fn ((fn ((ref str) u16) car)) u16)` emits `fn measure<'a>(constructor: impl FnOnce(&'a str, u16) -> Car<'a>) -> u16`, and `(fn ((fn () car)) u16)` emits `fn measure<'a>(constructor: impl FnOnce() -> Car<'a>) -> u16`. Each accepts a closure whose car borrows a capture. It stays elided when a call of it in the body passes a borrow of a local, or when the body passes it to a parameter that stays elided. A trait or impl method, with or without a receiver, names an `iterator` parameter and a function-typed parameter by the same rules, except that a method with a receiver keeps a function-typed parameter whose call can return the result elided when the parameter's type takes a borrowed argument and no parameter of the method takes the shared lifetime, so the parameter and the result share the receiver's lifetime. A receiver method's function-typed parameter with no borrowed argument whose call can return the result takes the shared lifetime with the result, as in a function: `(pick (fn ((ref self) (fn () car)) car))` in a trait emits `fn pick<'a>(&self, _: impl Fn() -> Car<'a>) -> Car<'a>;`. The trait's declaration and every impl agree, because each impl must match the declaration: a parameter takes the shared lifetime when the trait's default body or any impl lets its call reach the result, and stays elided when any of them calls it with a borrow of a local. `(build (fn ((fn ((ref str) u16) car) (ref self)) car))` in a trait emits `fn build<'a>(_: impl Fn(&'a str, u16) -> Car<'a>, _: &Self) -> Car<'a>;`, and `(park (fn ((fn () car) (ref self)) u16))` emits `fn park<'a>(_: impl Fn() -> Car<'a>, _: &Self) -> u16;`. When one body calls the parameter with a borrow of a local and another returns its call's result, the second body still takes the shared lifetime, so rustc rejects the impl. A returned borrow of a local value is an error, `L0510`.
- **Stored borrows.** A parameter whose borrow the body stores into another parameter shares that parameter's lifetime, as a returned borrow shares the result's. A call stores an argument when the argument's parameter type appears inside the referent of a `mut` parameter of the callee, as `a` does in `push!`'s `(fn ((mut (vec a)) a) unit)`, or when the callee's body stores it. The stored parameter and every elided borrow inside the target's referent take the shared lifetime `'a`, and the target's own reference stays elided, so the caller can use the target again: `(defn add-car! (cars car) (push! cars car))` with a borrowed struct `car` emits `fn add_car<'a>(cars: &mut Vec<Car<'a>>, car: Car<'a>)`. A stored value that borrows from no parameter, such as a struct of literals, adds no lifetime. A tail call or `recur` that passes one parameter's borrow as another parameter's next value stores it in that parameter, so both parameters take the shared lifetime in their whole types, because the loop assigns one to the other: `(recur years spare label (- count 1))` on two `(mut (vec i64))` parameters emits `fn fill<'a>(mut spare: &'a mut Vec<i64>, mut years: &'a mut Vec<i64>, label: &str, mut count: i64)`. A swapped parameter that also stores a borrow keeps rustc's lifetime error, because one shared lifetime cannot type both its reference and its elements. A parameter that the call passes unchanged, such as `label`, keeps its own lifetime.
- **Closures.** A closure's body decides its kind. A closure that moves a capture or a field of one, calls a captured `FnOnce` closure, or passes a capture to a parameter that owns it, unless its template borrows it, is `FnOnce`. A struct update moves its base's fields by the rule under Fields, so a closure that updates a capture moves it only when the update leaves out a field that is not Copy: with `(defstruct garage :name string :city string :size u16)` bound to `g`, `(fn () (assoc g :name (str "x") :city (str "York")))` stays `Fn`, because `size`, the only unlisted field, is Copy. Calling a local `FnOnce` closure moves it, so a second call is a use-after-move error, `L0501`. One that changes a capture, calls a captured `FnMut` closure, or passes a capture to a `mut` parameter, is `FnMut`. Every other closure is `Fn`. A closure that another closure returns takes its kind from its own body by the same rules, and a call of the outer closure gives a closure of that kind, through any number of calls and `let` aliases: with `(let make-getter (fn () (fn () (keep model))))`, `(twice (make-getter))` is `L0509` as passing the inner closure directly is, and a second call of `(let getter (make-getter))` is `L0501`. A call of a function whose signature writes its result's kind gives a closure of the written kind, whatever kind the body returns: with `(type make (fn () (fn-once () string)))`, a second call of `(let getter (make))` is `L0501`. A function parameter takes the weakest kind every use allows: a call that is its only use allows `FnOnce`, any other call needs `FnMut`, and a pass needs its callee's kind. A call inside a local closure bound with `let` counts once for each use of that closure, so a closure called twice, or passed where a `FnMut` is expected, makes the parameter `FnMut`. A call or a pass through a `let` alias of a parameter or of a local closure is a use of what it aliases, so `(let again step)` followed by two calls of `again` calls `step` twice. A call inside a lambda passed where a `FnMut` is expected, such as the callback of `for-each`, can run many times, so it makes the parameter `FnMut`. Uses in different branches of an `if` or a `match` are not counted together, and a pass that can come back to the same parameter through recursive calls allows every kind. A pass moves the parameter, so it must be the last use. A call of a `FnMut` binding whose argument mentions the same binding binds that argument and every argument before it with `let` before the call, because Rust borrows the callee mutably before it evaluates the arguments. A signature writes `(fn-mut ...)` or `(fn-once ...)` to pin a kind, and that written kind is exact; a plain `(fn ...)` leaves the kind to inference. The prelude's `rust` templates write the kind their Rust body requires. `Fn` meets `FnMut` and `FnOnce`, and `FnMut` meets `FnOnce`, as in Rust. A closure stronger than the parameter accepts is an error, `L0509`; its label names the capture and its help says to clone it or to borrow it. A function-typed parameter emits `impl Fn`, `impl FnMut` or `impl FnOnce`, and so does a returned closure behind its return type. A function-typed parameter of a closure bound by `let` without a written type, or with a type written only by `the`, emits a borrowed trait object instead, unless the closure stores it or the body uses the binding other than by calling it, because a Rust closure's parameter cannot be `impl Fn`: `&dyn Fn` for an `Fn` parameter and `&mut dyn FnMut` otherwise, because Rust cannot call a borrowed `FnOnce`. Each call of the binding borrows that argument, so `(let apply (fn (action) (action year)))` called as `(apply (fn (n) (record n)))` emits `let apply = |action: &mut dyn FnMut(i64)| action(year);` and `apply(&mut |n: i64| record(n))`, and the binding accepts a different closure at each call. When such a parameter's result holds a borrow and its parameters hold none, the result takes the enclosing function's shared lifetime, or `'static` when the function has none, because a Rust closure cannot declare a lifetime: a `(let show (fn (make) (make)))` in `main` emits `|make: &mut dyn FnMut() -> Car<'static>|`, and the same closure in a function with the shared lifetime emits `Car<'a>`. A call that passes a closure whose car borrows a local or an unshared parameter gets rustc's lifetime error. A parameter of a closure bound by `let` without a written type whose borrow can reach the closure's result takes the same lifetime, because Rust gives an elided closure parameter a lifetime of its own that the result cannot borrow: `(let first (fn ((s (ref str))) s))` in `main` emits `let first = |s: &'static str| s;`, and the same closure in a function with the shared lifetime emits `|s: &'a str|`. A call of such a closure that passes a borrow of a local value or of a temporary to a parameter whose borrow the closure returns is an error, `L0512`, because neither lifetime outlives the local: `(let first (fn ((items (ref (vec i64)))) (at items 0)))` called as `(first years)` with a local `years`. Its help suggests `defn`, whose result can borrow from its argument. The closure returns a parameter's borrow when the parameter reaches its result through the forms that the `L0510` check follows, except that a call's result reaches the parameter when any argument that the result can borrow reaches it, as `maybe` reaches `(unwrap-or maybe "none")`, because the emitted closure gives the parameter that lifetime whenever it flows into the result. `L0510` reports a call only when every such argument borrows a local, because a generic callee's result can borrow from one of its arguments alone. A call through a `let` alias of the closure is a call of the closure: `(let again first)` followed by `(again years)` is `L0512`, its label and help name `first`, and a label on each alias says what it aliases, as ``` `again` is `first` ``` does. A call that passes a borrow of a parameter that does not take the shared lifetime gets rustc's lifetime error. Such a parameter is at most `FnMut`, so a `FnOnce` closure passed to it is an error, `L0509`. A function-typed parameter of any other closure emits the `fn` pointer type, which accepts only a closure that captures nothing. A function-typed parameter that the body stores in a struct field or an enum payload whose type holds a function type, directly, through `let` bindings of it, inside a stored value such as `(some f)`, or as a result of a branch of an `if` or a `match` or the last form of a `do`, emits its own `fn` pointer type instead, as `make_sorter(key: fn(&User) -> u16, label: &str)` does, because Rust cannot coerce an `impl Fn` parameter to a `fn` pointer. Its argument is a named function or a closure that captures nothing. A capturing closure written where such a field or payload is stored is an error, `L0511`. A function type inside a field's or payload's type, as in `(option (fn () i64))`, also emits a `fn` pointer, so a capturing closure stored inside such a field, as the payload of a variant such as `some` or as an argument that a `rust` template's result holds, such as an element of `vec` or a side of `pair`, is the same error. A closure bound by `let` and then stored counts as written where it is stored, through any number of `let` bindings, and the error points at the closure. So does a closure that a branch of an `if` or a `match`, or the last form of a `do`, returns into such a field or payload, as in `(->spare :maker (some (if fresh (fn () (+ base 1)) count-doors)))`. The check also covers a closure passed to a parameter that its function stores in such a field, as a positional constructor call such as `(->sorter (fn (someone) (+ (:age someone) bonus)) "by age")` does, and a closure passed to a closure bound by `let` that stores its parameter. A closure returned from a function is a `move` closure. A closure that only borrows or copies its captures is Copy and can be passed more than once. A closure that passes a capture to a `{&N}` placeholder borrows it. A local `FnMut` closure emits as `let mut`, because Rust takes such a closure mutably.
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

**A definer that fails reports one error.** `defn`, `defmacro`, `defrule` and `defstruct` declare the names they define, with `declare-definition`, `declare-macro-definition` and `declare-type-definition` from the `code` module, before their own checks. When the definer's top-level call fails, the expander binds each declared name, so `(area 1)` after a failed `(defn area "doc" (w) w)` adds no `L0201`, and a use of the type or the constructor after a failed `defstruct point` adds none either. A definer that fails inside a body binds its declared value name as a local for the rest of the body, so `(defn outer () (defn inner "doc" (x) x) (inner 1))` reports the `L0328` alone, and a call of a failed macro there skips its arguments too. A later definition of a failed name replaces it with no `L0203`, so a fixed copy below a broken one reports only the broken one. Macro code that calls a failed name reports `L0308`, as it does for any definition with errors. A user definer declares its names the same way, so its failures report once. A call of a failed function still checks its arguments, so `(area (missing-width) 2)` reports the `L0201` for `missing-width`, while a call of a failed macro skips its arguments, which need not be code the program could run. A misspelled name, such as `aera`, still reports `L0201`, with the failed name as its suggestion. A name that no declaration reaches stays unbound: `(defn area)` does not match the shape of `defn`, so the call fails before `defn` declares `area`, and `(area 1)` reports `L0201`.

The prelude also provides `defn` for a function definition, `the` for a typed value, `->` and `->>` for threading a value through calls, `let-try` and `try->` for propagating errors, and `assoc` and `assoc-in` for changing a value. `defn` with a plain parameter list expands to `def` and `fn`. `->` passes the value as the first argument of each call, and `->>` as the last.

### Keyword parameters

A `defn` parameter list whose first element is a keyword declares keyword parameters, read like `defstruct` fields. Each `:name` takes an optional type, so `(:x :y)`, `(:x i64 :y i64)` and a mix declare two parameters. A parameter keyword whose name ends with `:` is `L0332`, as a field keyword is. A type is never a keyword. The body names each parameter without the `:`. A list headed by `defaults` is always the defaults clause, even right after a keyword, and it comes last, as in `defstruct`. Any other parameter list keeps its plain meaning.

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

A `defrule` clause is a pattern and a template. In a pattern, `name ...` matches zero or more forms, at any depth, and `(quote name)` matches the symbol `name` itself. The repeated variables that a template fills together under one `...` must have the same length: with `(defrule zip (((a ...) (b ...)) (vec (pair a b) ...)))`, the call `(zip (1 2) (4 5 6))` is `L0330`, with a caret on the `6` that has no matching `a`. A `defmacro` parameter list is a pattern in the same language.

Write a macro with `defrule` when its expansion follows from the shape of its call. Use `defmacro` only for a macro that computes, such as one that counts forms or builds names. `dotimes` and `case` need no computation:

```lisp
(defrule dotimes
  (((index count) body ...) (for-each (range 0 count) (fn (index) body ...))))

(defrule case
  ((value clause ...) (do (let subject value) (case-clauses subject clause ...))))
```

One implementation per name keeps the compiler small. Compile-time twins of the prelude's Rust templates would disagree with Rust on integer width, overflow, division by zero and floats. Most macros need no computation when written with `defrule`.

### Docstrings

A docstring is a string literal that documents a definition. `defn` and `defmacro` take it as the first body form, and only when another form follows it:

```lisp
(defn twice (x) "Doubles a number." (* 2 x))   ; documented
(defn greeting () "hi")                         ; returns "hi", no docstring
(defn pair-of () "a" "b")                       ; documented "a", returns "b"
```

`defstruct` and `defrule` take the docstring as the first form after their header: `(defstruct point "A point on a grid." :x i64 :y i64)` and `(defrule swap "Swaps the two arguments of a call." rules...)`. A string in place of the parameter list of `defn` or `defmacro` is `L0328`. `def` takes the docstring between the name and the value, again only when another form follows it: `(def max-retries "How many times a check reruns." 1)` documents `max-retries`, and `(def greeting "hi")` binds the string. The `def` that programs see is a prelude macro over the kernel `def`, so the kernel form keeps the shape `(def name value)`. A user documents a definition through these five definers. `(document header text)` documents a definition that no definer documents, such as a kernel form or a `def` in a prelude module, or one whose definer's header shows no call shape, such as a `defrule` operator. A program imports it with `(use prelude/macros document)`. The header is the term's call form, such as `(document (str part ...) "Builds a string from its parts.")`, or the bare name of a term that takes no arguments, and the page shows it as written. The header's head names the term, so the name is never written twice. The prelude documents `str`, `get`, `push!`, `defrule`, `defmacro`, `template`, the exports of `prelude/number` and `prelude/logic`, and the exports of `prelude/text`, `prelude/io`, `prelude/vec`, `prelude/iterator`, `prelude/sequence`, `prelude/option`, `prelude/result`, `prelude/types`, `prelude/pair`, `prelude/ownership` and `prelude/map` that no `defmacro` or `defn` defines this way.

The prelude's string lookups, `docstring`, `header`, `page-body` and `doc-page`, expand to string literals at expansion time, so `(def twice-doc (docstring twice))` emits `const TWICE_DOC: &str = "Doubles a number.";`, and a program's emitted Rust is the same with and without its docstrings:

| Lookup | Returns |
| --- | --- |
| `(docstring term)` | The docstring. |
| `(header term)` | The first line of the definition as written, such as `"(defn twice (x))"`, or the bare name of a term without a written header. |
| `(page-body term)` | The page after its header: a blank line, then the docstring, or `No documentation.` for a term without a docstring. |
| `(doc-page term)` | The page: the text of `(header term)` followed by the text of `(page-body term)`. |

`(doc term)` prints the page to standard output, as `rslisp doc` prints it, and returns `unit`: it expands to `(println (doc-page term))`, so `(doc twice)` at the prompt prints the page as text, and `(def p (doc twice))` binds `unit` and prints the page when a form first reads `p`. Write `(doc-page term)` to keep the page as a string.

A lookup finds the docstring in the module that defines the binding `term` refers to, so an imported or re-exported term needs no other export. Each definer stores the docstring of a documented definition as a note under `docstrings` in its module's notes, with `add-module-note`, and defines no other name, so a program may define `doc/twice` beside a documented `twice`. An undocumented definition stores nothing. A lookup reads the notes of the defining module with `module-notes` and takes the note whose name refers to the same binding as `term`, so a documented name that a macro introduces never answers for a user's name of the same spelling. A lookup reads the docstring of the type binding only when the value binding has none, so a documented struct keeps its page beside an undocumented function of the same name.

Every bound term has a page, as in `go doc`. A term without a docstring gets its header, a blank line, then `No documentation.`. The lookup rebuilds the header of every page from the macro call that defined the term, which `definition-call` returns, or `type-definition-call` for a documented struct. The header is as many leading forms of the call as the shape that the call's macro states with `header-shape` (see Header shapes), or the whole call when the call is shorter than the shape, so `(doc-page twice)` after `(defn twice (x) (* 2 x))` is `"(defn twice (x))\n\nNo documentation."`. A lookup reads the shapes of the module that defines the definer only, so a user definer states its shape in its own module. When the call's macro states no shape, the lookup expands the call one step with `expand-call` and tries the expansion's macro, until a macro states a shape, for at most 32 expansions. A definition made through a user macro thus shows the same header with or without a docstring:

```lisp
(defrule my-defn ((n p b ...) (defn n p b ...)))
(my-defn twice (x) (* 2 x))
(def twice-page (doc-page twice))   ; "(defn twice (x))\n\nNo documentation."
```

With `(header-shape (my-defn name parameters))`, the same page starts with `(my-defn twice (x))`. The walk ends with the term's bare name after 32 expansions, at a call whose expansion fails, or at an expansion that is not a list headed by a bound macro's symbol, such as a call of an unbound name, or one that defines several names in a `do`. A plain `def` is a call of the prelude `def`, whose shape is `(def name)`, so `(doc-page limit)` after `(def limit 10)` is `"(def limit)\n\nNo documentation."`, and so is the page of a `def` that a user macro writes. A term that no macro call defined, such as a parameter, takes its bare name as header. `prelude/prelude.rslisp` documents each kernel form, each kernel constant and each kernel type except `fn`, `ref`, `mut`, `char` and `str` with `document`, beside the kernel's header shapes, and a lookup of a kernel name reads the notes of `prelude`, the module that exports the kernel, so `(doc-page if)` starts with `(if test then else)`. A kernel type with no form row has its shape as header, as `(with type type)`, or its bare name, as `i64`, and a constant's header is its name. The page of `fn`, `ref` or `mut` is the page of the kernel form of that name. The page of `char` or `str` is the page of the prelude value of that name. An unbound term is `L0201`, with the closest name. `(docstring term)` of a term without a docstring is `L0327`.

A docstring is Markdown. A lookup removes the indentation that every written continuation line shares, as Python's `inspect.cleandoc` does, and drops blank lines at both ends. Write docstrings to these rules, which no compile error enforces:

- The first line is one complete summary sentence.
- Lines stay within 80 columns.
- Continuation lines are indented to the body.
- Markdown conventions apply.

`rslisp doc [--types] [--file <entry>] <term>` prints the page that `(doc-page <term>)` returns, for a prelude term or, with `--file`, for a term the entry module defines or imports. It expands `(header <term>)` and `(page-body <term>)` and prints the first followed by the second, so it can color the header apart from the docstring, as [Colors](#colors) states.

`--types` also type checks the program and prints the inferred type of the definition that the page describes above the page, as the line that `rslisp types` prints: `--types twice` prints `(type twice (fn (i64) i64))`, then the page. A macro that answers for a function, such as `->point` or a `defn` with keyword parameters, gets that function's type under the macro's name. A term with no type of its own, such as a struct, another macro or a kernel form, gets the page alone. A documented struct `point` beside an undocumented function `point` gets the struct's page, so it gets no type line. When the program does not type check, `--types` prints the page with no type line, the errors on stderr, and exits 1. `(doc term)` and `(doc-page term)` never show a type, because a lookup expands before the type checker runs.

### Header shapes

`(header-shape (form name ...))` states the header of a form's calls: the leading forms that belong with the form's name, before its body. `(header-shape (defn name parameters))` says that the header of a `defn` call is its name and its parameter list. `doc` shows the header of an undocumented definition (see Docstrings), and `rslisp fmt` reads the same shapes to keep a header on a call's first line (see Source formatting).

A `:docstring` after the shape states that the form takes a docstring as the first form after its header, when another form follows it: `(header-shape (defn name parameters) :docstring)`. A shape adds itself, such as `(defn name parameters)`, to its module's notes under `header-shapes`, and a `:docstring` adds the form's name under `docstring-forms`, so macro code reads both with `module-notes`. A `:one-per-line` after the shape states that the formatter does not fill runs of atoms in a broken call, so each later form takes its own line apart from keyword pairs, as the prelude states for `enum`. The formatter does not expand, so it reads the `header-shape` forms themselves.

The prelude states the shapes of its four definers, of `def` and of `document`, each with `:docstring`, of its macros whose calls have a header and a body, such as `(header-shape (loop bindings))` and `(header-shape (let-pair names value))`, and of the kernel forms whose header stays on the first line when the call breaks, such as `(header-shape (match value))` and `(header-shape (impl trait type))`. `prelude/prelude.rslisp` states the kernel's shapes, beside its re-export of the kernel. `prelude/test` states `(header-shape (deftest name) :docstring)`. The kernel's shapes include `(header-shape (if test))` and `(header-shape (do))`, so an `if` or a `do` that breaks indents its branches or forms 2 columns, and `prelude/macros.rslisp` states `(header-shape (fail message))` for the `code` module's `fail`.

A user macro states its own shape with `(use prelude/macros header-shape)`, below the macro's definition. A shape that is not a list that starts with a name is `L0329`. The name must refer to a binding where the shape is written, so a misspelled name is `L0201` with the closest name, and so is a shape above the definition of the macro it names, as a call above that definition is. Any form after the shape other than `:docstring` and `:one-per-line`, each at most once, is `L0331`.

### The `code` module

The compiler provides the `code` module for macro bodies. Code that uses it exists only at compile time. Importing `code len` hides the runtime `len` in that module, so a call to `len` on a program value there is `L0412`.

| Built-ins | Purpose |
|---|---|
| `first rest list prepend concat len empty? nth take drop` | Build and take apart lists of forms. |
| `symbol? keyword? list? string? number?` | Test what kind of form a value is. |
| `symbol-name`, `keyword-name`, `keyword-field`, `field-keyword`, `keyword-trim-end`, `(symbol context part...)`, `(string part...)` | Read and build names and strings. |
| `escape-string`, `escape-char`, `escape-format`, `string-strip-suffix`, `string-strip-prefix` | Escape a string for a Rust string literal, a Rust character literal or a format string in a template, and drop a suffix or a prefix. `escape-string` and `escape-format` write a control character as a `\u{..}` escape, so `"count\u{1e}"` stays readable. |
| `(string-lines text)`, `(string-trim-start text)`, `(string-length text)`, `(string-drop count text)` | Split a string at each newline into a list of strings, drop its leading whitespace, count its characters, and drop its first `count` characters. A `count` past the end is an error. |
| `(same-binding? left right)` | Test whether two symbols refer to the same binding. |
| `(definition-symbol term part...)` | Build a symbol, as `symbol` does, with the scopes of the module that defines the binding `term` refers to. An unbound `term` is an unbound-name error at `term`. A kernel form, kernel type or kernel constant gives a symbol with the scopes of `prelude`, the module that documents it. Any other binding that no module defines, such as a local, gives a symbol with no scopes. |
| `(type-definition-symbol term part...)` | Build a symbol as `definition-symbol` does, from the type binding `term` refers to, even when `term` also has a value binding. A `term` with no type binding gives a symbol with no scopes. |
| `(definition-call term)` | Return a list that holds the macro call, as written in the source, that defined the module-level binding `term` refers to: the innermost macro call whose written form holds the binding's name. Its symbols carry no scopes. A binding that no macro call defined, such as a plain `def`, a local or a kernel form, gives an empty list. An unbound `term` is an unbound-name error at `term`. |
| `(type-definition-call term)` | Return a list that holds the macro call, as `definition-call` does, that defined the module-level type binding `term` refers to, even when `term` also has a value binding. A `term` with no type binding, or a type that no macro call defined, such as a kernel `struct`, gives an empty list. |
| `(expand-call call)` | Expand the macro call `call` one step, as Clojure's `macroexpand-1` does, and return a list that holds the expansion. A `call` whose head is not a macro, or whose expansion fails, gives an empty list. The expansion adds no notes and reports no errors. |
| `(add-module-note key note)` | Add the form `note` to the notes under the symbol `key` of the module being expanded, and return unit. It changes the expander's state: a note stays only while the macro call that added it succeeds and the expander keeps its expansion, so a note from a failed call or a discarded trial is removed. |
| `(module-notes context key)` | Return the list of notes under the symbol `key`, in the order added, of the module whose scopes the symbol `context` carries exactly, as `definition-symbol` builds it. A `context` with no module's scopes gives an empty list. |
| `(declare-definition name)`, `(declare-macro-definition name)`, `(declare-type-definition name)` | Declare the form `name` as a value name, a macro name, or a type name, that the expansion of the calling macro defines, and return unit. `name` keeps the scopes of the call, as the definition the expansion makes keeps them. When a macro call fails, the expander binds each declared symbol it can bind: every name at module level, and the first value name in a body, so a later use of the name reports no error of its own. |
| `(form-location form)` | Return a string that holds the location of `form` as `<file>:<line>:<column>`, with the file relative to the root of `use`, such as `"text/format.rslisp:20:3"`. A form that a macro introduced takes the location of the macro call. |
| `(program-modules)` | Return a list that holds one symbol per module the program loads, other than the prelude's modules, `kernel` and `code`, in the order the modules start loading. Each symbol's name is the module's path, and the symbol carries the module's scopes, as `definition-symbol` builds them, so `module-notes` reads that module's notes. |
| `(module-exports path)` | Return a list that holds a symbol per distinct name that the module at `path` exports, sorted by name, where `path` is a symbol that names the module as `use` writes it. Each symbol carries the module's scopes, so `same-binding?` compares it with a name a program writes. A module that the program has not loaded is `L0204`. |
| `(closest-name name names)` | Return a list that holds the symbol of `names` closest to `name`, other than `name` itself, ranked as the compiler ranks a misspelled name, or an empty list when none is close. |
| `(fail "message" form)`, `(fail "message" form "help")`, `(fail "message" form "help" description)` | Stop expansion with an error at `form`, an optional help line that says how to fix the call, and an optional description of the error. |
| `number-add number-sub number-lt number-le number-gt number-ge` | Count and compare positions in code. They take integers. |
| `value-eq bool-not` | Compare forms and negate booleans. |

`first`, `rest`, `nth`, `take`, `drop`, `len` and `empty?` take constant time: a list that `rest`, `take` or `drop` returns shares its items with the list it reads. `concat` appends to its first list in place when `list`, `concat` or `prepend` built that list and no other `concat` has grown it since. Otherwise it copies the first list too. So a loop that builds a list with `(recur (concat built (list item)) ...)` takes linear time. `prepend` copies its list.

A keyword is a symbol whose name is `:` followed by at least one character, such as `:year`. `keyword?` tests for one, and `keyword-name` returns its name without the `:`, so `(keyword-name (quote :year))` is `"year"`. `keyword-field` returns the field symbol, so `(keyword-field (quote :year))` is `year`, with the keyword's location and hygiene, so an error about the field points at the keyword the user wrote. `field-keyword` is its inverse: `(field-keyword (quote year))` is `:year`, with the field's location and hygiene. `keyword-trim-end` drops every `:` that ends a keyword's name, so `(keyword-trim-end (quote :year::))` is `:year`, with the keyword's location and hygiene. A keyword whose name is only `:` characters, such as `::`, becomes the symbol `:`, which is not a keyword. The compiler states the `:` marker once, so macro code never compares names with it.

A `fail` error takes the code `L0301` unless its description names another. The description is a list of `:part value` pairs, as in `(template (:code repeated-parameter :label "given again here" :earlier (unquote first) :earlier-label "first given here"))`. `:code` names a macro-stage diagnostic by its name in `expand/src/diagnostic/code.rs`, lowercased with `-` for `_`, so a prelude error gets its own code without restating the code list. A code of another stage is an error, because a code's first digit names the stage that reports it. `:label "text"` names the label of `form`; a failure with no `:label` labels `form` "here". `:earlier` marks `form` as a repeat of an earlier form and needs `:earlier-label "text"`, which names the label of that earlier form. `:suggest` names a symbol that replaces `form`, and the help line introduces the replacement, as in `(fail message key "a parameter with a similar name exists" (template (:code unknown-parameter :suggest (unquote closest))))`. The message, the help line and the labels are the macro's own words.

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

## Evaluation

`rslisp eval <file> <forms>` checks a module, then runs forms once in its scope ([RFC 0047](../rfcs/0047-first-repl.md)). `<forms>` is one argument that holds one or more forms.

- `eval` reads the module's text from `<file>`. With `--stdin`, it reads the text from standard input, and `<file>` still names the module.
- `eval` checks the text as `rslisp check` does, so a module with no `main` checks. It resolves `use` from `--root <directory>`, which defaults to the directory of `<file>`.
- The forms are the body of a generated `main` in the module's scope. That `main` ends with `(do)`, so every form is a statement.
- The generated `main` is the program's entry. No source name refers to it, and it takes the Rust name `main`. A module's own `main` is checked as `check` checks it, does not run, and becomes `main_2` in Rust if a form calls it.
- A form sees every definition of the module and of the prelude. Every rule of a body holds, so `(let x 5) (* x 2)` prints `10` and a definition as a form is `L0208`.
- The forms run once, in order, in one program, so one call pays one rustc run.
- Nothing persists between two runs. A `def` whose value is computed runs again in each run that uses it.
- The program reads `eval`'s standard input, as under `run`.

**A form's result prints by its type:**

| Result | Output |
| --- | --- |
| `unit` | nothing |
| a type that the parameter of the prelude's `prn` accepts | the value, as `prn` prints it |
| any other type, such as a closure or an `iterator` | a `note` on standard error that names the type and the trait it lacks |
| a type still open after inference, such as `none` | `L0407` on the form, with a fix that wraps the form in `the` |

**The forms are their own source, named `<eval>`, so a diagnostic points at the form.** `--line <n> --column <n>` give the position where the forms start in the module's text. Lines and columns count from 1, and columns count UTF-8 bytes. The forms then MUST be the module's text at that position, or `eval` exits 2. Their diagnostics carry the module's name and the forms' own lines.

**With `--line` and `--column`, each form's place in the module decides what happens to it:**

| The form | `eval` |
| --- | --- |
| is a definition at the module's top level, or inside a top-level `do` | checks the module, prints a `note` that the module defines the form, and runs nothing for it |
| is a top-level `do` that holds a definition beside other forms | reports `L0207` on each other form, as `check` does |
| is any other form at the module's top level | leaves the form out of the module check and runs it |
| sits inside a definition | keeps the form in that definition for the module check and runs it again, where it sees only the module's definitions and the prelude |

**If the module has an error, `eval` reports the module's diagnostics and runs nothing.**

| Exit code | When |
| --- | --- |
| 1 | the module or the forms have errors |
| 2 | a usage error |
| 0 | a closed output stops the program, as under `run` |
| 128 + N | signal N stops the program |
| the program's code | otherwise |

**`eval --json` writes one JSON object per line on standard output, so an editor can tell output, values and diagnostics apart** ([RFC 0049](../rfcs/0049-eval-amendments.md)). With `--id <text>`, every object also carries `id` with that text. `--id` requires `--json`.

| Object | Carries |
| --- | --- |
| `out`, `err` | the program's standard output and standard error, as they stream |
| `value`, with `form` | one form's result, as `pr-str` prints it, and the number of the form from 1, definitions included |
| `diagnostics` | the diagnostics of one report, notes included |
| `status`, with `exit` | `"done"` and the exit code, as the last object |

- Each diagnostic carries `severity`, which is `error`, `warning` or `note`, `code`, which a note lacks, `message`, `file`, `line`, `column`, `end_line`, `end_column` and `rendered`, the plain text that `eval` prints without `--json`.
- Lines and columns count from 1, and columns count UTF-8 bytes, as `--line` and `--column` do. The end is one past the span.
- A form that prints its result becomes `(print-value form)`, where `print-value` is one generated function, `(fn (value) (println "<marker>" (pr-str value)))`, with `println` and `pr-str` resolved in the prelude's scope. It is generic over the value, so one function prints forms of every type, and it emits one `println!` with no added `String`: `(square 3) (->point :x 1 :y 2)` emits `print_value(&square(3)); print_value(&Point { x: 1, y: 2 });` through `fn print_value<A: crate::rt::Show>(value: &A)`. With one printed type the function takes that type. The argument runs before the marker prints, so a form's own output comes first, and its temporaries live until the value prints.
- `eval` sends the program's standard output as `out` up to a marker, and the rest of that line as `value`. It holds back an end of the output that could start a marker until the next read shows whether it does.
- A `unit` result and a result without the printing bound get no marker and no `value`. The second gets its note in `diagnostics`.
- A usage error, such as a missing file or forms that are not the module's text at `--line` and `--column`, writes no JSON object: it prints to standard error and exits 2. A build environment that fails, such as a missing `rustc`, prints its error to standard error and ends with `status` and exit 2.

### The prompt

`rslisp repl` reads inputs from standard input. `rslisp` with no command starts the same prompt, and takes the global arguments such as `--color`. Its only state is a buffer of the definitions entered so far, as text. Each form is one input, so a line of three forms is three inputs, and each input is its own source, named `<input N>`.

- The expander classifies each input. An input that a file accepts at its top level, `use` included, is a definition. Any other input is an expression.
- A definition replaces every earlier input that defines or imports a name it defines or imports, and takes the place of the first one. The redefinition takes effect, even when it drops a name ([RFC 0051](../rfcs/0051-repl-redefinition.md)).
- An expression runs as one form of `eval` against the buffer, and its result prints by the table above, so `(infer (vec 1 2))` prints the string `"(vec i64)"`, quoted. A program that panics or exits nonzero gets a `note` with its exit code, and the buffer keeps its definitions.
- `--root <directory>`, which defaults to the current directory, resolves `use`.
- While the reader reports an unclosed list, `L0101`, or an unclosed string, `L0103`, the prompt reads the next line into the same text. Any other reader error rejects the text at once.
- On a terminal, the prompt is `> `. Piped input shows no prompt.
- The end of input quits with exit code 0, also after failed inputs. An unclosed list or string at the end of piped input is reported first.

**On a terminal, the prompt edits the line and keeps a history** ([RFC 0047](../rfcs/0047-first-repl.md)). Piped input reads plain lines. So does a terminal whose `TERM` is `dumb`, `cons25` or `emacs`, which the line editor does not support: there the prompt shows `> `, a continuation line shows two spaces, an unclosed list or string at the end of input is reported, Ctrl-C while typing drops the line, and Ctrl-C while rustc, rustfmt or the program runs acts as below.

| Key | Effect |
| --- | --- |
| arrows, Home, End, Backspace, and the Emacs keys such as Ctrl-A and Ctrl-E | move and edit within the text |
| Up and Down | step through the history |
| Enter | ends the text while the reader reports no unclosed list or string |
| Enter inside an unclosed list | inserts a newline and two spaces into the text, at any depth |
| Enter inside an unclosed string | inserts a bare newline into the text |
| Ctrl-C while typing | drops the text, finished or not, and shows a new prompt |
| Ctrl-C while rustc, rustfmt or the program runs | stops it, prints `interrupted`, skips the rest of the text's inputs, and keeps the definitions |
| Ctrl-C while the front end handles an expression | lets it finish, skips rustc, prints `interrupted`, and skips the rest of the text's inputs |
| Ctrl-D at an empty prompt | quits with exit code 0 |
| Ctrl-D in a text that is not empty | deletes the character under the cursor, if any, and never quits |

- The two spaces after Enter are text, not a prompt: the input holds them, and a diagnostic counts them in its columns.
- A bracketed paste of several lines shows once, as one text, with no spaces inserted, and Enter ends it. Lines typed while an input runs wait for the next prompt.
- The history holds the last 1000 texts, in `$XDG_STATE_HOME/rslisp/history`, or in `~/.local/state/rslisp/history` when `XDG_STATE_HOME` is unset or empty. The prompt adds each text to the file as it is entered.
- If neither variable is set, or the file cannot be read or written, the prompt works without history and prints one `note` that says why.
- If the prompt cannot catch Ctrl-C, it prints one `note` that Ctrl-C will end the session.
- A rustc or rustfmt that Ctrl-C stops reports no diagnostic and no note, under every command, whether it dies of the signal or catches it and fails, as rustc does. A command other than `repl` then exits with code 130, as for a program that Ctrl-C stops.

**A definition takes effect, and each earlier input that no longer checks is set aside.** The prompt checks the buffer as `check` checks a file, and locates each error at one input:

1. An error whose primary label, or one of whose `in this macro call` labels, lies in the new input is the new input's.
2. Otherwise the error is located at the input of its outermost `in this macro call` label, then at the input of its primary label, then at the earliest input whose `use` reaches the file of its primary label.
3. An error located at no input, as one in the prelude, is the new input's. Secondary labels never decide.

If an error is located at the new input, the prompt rejects the input, prints those errors, and keeps the buffer as it was. If errors are located at earlier inputs, the prompt sets each of those inputs aside and checks again. A round removes at least one input, so the check ends. Once the buffer checks, the new input joins it, and the prompt prints one `L0249` for each input set aside and for each replaced input that binds a name the new input does not, with the new input's own warnings, as one report.

`L0249` names the input's names, at most three and then `and N more`, or the input's source when it bound none. It carries the error that set the input aside as its own label, a note and helps, or for a dropped name the note that the new input replaced it, and ends with the help to enter the names again.

**The prompt records each name that leaves the buffer,** in its namespace, and an `L0201` for a recorded name gets a note: `` `<input 4>` set aside `greet`: cannot find `karl` in this scope ``, or, for a name an input stops binding while it still checks, `` `<input 3>` no longer defines `foo` after `<input 4>` ``. A rejected input changes no record, and its `L0201` for a name of an input it would set aside reads `would set aside`. After each definition that joins, the prompt forgets the record of each name the buffer binds.

An expression never changes the buffer. After a module file changes on disk, an expression's error located at an earlier input gets the help `enter a definition to set aside the inputs that no longer check`, and the next definition sets those inputs aside.

**The prompt differs from a file in three ways:**

1. An input that is not a definition runs once, as the body of a generated `main`.
2. A definition replaces the earlier definition of the same name, where a file reports `L0203`, and sets aside each earlier input that no longer checks.
3. Each definition is checked when it is entered, so it comes after the definitions it uses. Mutually recursive functions go in one input, as `(do (defn even? ...) (defn odd? ...))`.

### Colors

**The CLI decides once at start-up, for each output stream, whether to color it** ([RFC 0050](../rfcs/0050-repl-colors.md)). The diagnostics and the help follow the same decision. The first step that applies decides:

1. `--color always` or `--color never` decides.
2. A non-empty `NO_COLOR` turns color off.
3. A non-empty `CLICOLOR_FORCE` turns color on.
4. `CLICOLOR=0` turns color off.
5. A stream that is a terminal is colored when `TERM` is set and is not `dumb`, when `CLICOLOR` is set and is not `0`, or when `CI` is set.
6. Otherwise the stream is plain.

**`--color auto|always|never` is a global argument, so it MAY follow the subcommand.** The CLI reads it before it parses the other arguments, up to a `--`, so the help and a usage error follow it too. `auto` is the default. A value other than `auto`, `always` or `never` is a usage error, colored by `auto`'s rule.

**`eval --json` never colors, whatever the decision:** an editor parses each `rendered` field.

**The prompt colors two lines of its own:**

| Line | Color | Follows |
| --- | --- | --- |
| the prompt `> ` | bold bright blue, the style of the diagnostics' line numbers | standard output's decision |
| `note: the program exited with code N` | a diagnostic note | standard error's decision |

**On a terminal, the prompt colors the input as it is typed, by standard output's decision.** The colors key on the reader's tokens, so a partial line such as `(defn f "abc` colors too:

| Token | Color |
| --- | --- |
| a string, quotes included, or an unclosed string to the end of the text | green |
| a number, any atom that starts with a digit after an optional sign | yellow |
| a keyword, a symbol `:name` | cyan |
| a comment, `;` to the end of the line | dim |
| parentheses and every other symbol | plain |

**The prompt and `eval` without `--json` color each printed value with the same token colors, by standard output's decision.** `true`, `none` and every other symbol print plain, because the colors key only on the reader's tokens.

- When standard output decides to color, the program's standard output goes through a pipe. The CLI finds each value by the marker of `eval --json`, colors the value, and writes every other byte of the program's output unchanged, as it arrives, so a program's own escape codes and a line it prints before it reads input reach the terminal at once.
- When standard output decides not to color, the program keeps standard output, as under `run`, and each value prints plain. So `rslisp eval f '(g)' | less` prints the value plain, and a diagnostic on a terminal stays colored.
- The program's standard error never goes through a pipe and never takes a color.

**`rslisp doc` colors the header and the `--types` line with the token colors, by standard output's decision.** The docstring is Markdown and stays plain. At the prompt, `(doc term)` is the program's output, so the whole page prints plain.

## Tests

**`deftest` defines a test, and `assert` checks a value inside it** ([RFC 0055](../rfcs/0055-tests.md), [RFC 0056](../rfcs/0056-test-amendments.md)). Both are macros of the module `prelude/test`, which the always-loaded prelude does not import, so a program with no tests pays nothing for them. A test module imports them, as a Clojure test file requires `clojure.test`.

```lisp
(use prelude/test assert deftest)

(deftest pads-left
  (assert (= (pad-left "7" 3) "  7")))

(deftest trims-right
  "Removes trailing spaces, not inner ones."
  (assert (= (trim-right "a b  ") "a b ")))
```

- A test's name is a symbol that is not a keyword. A string before at least one more body form is the docstring, as for `defn`, and a failure prints its first line.
- A test's function ends with a unit form, so a test's value is dropped and a test passes when it returns. A test that returns an `err` passes: `assert` is what fails a test. An open type in a test's last form is `L0407` at that form under every command.
- A test sits at the top level of any module, beside the code it tests or in a module of its own. A `deftest` in a body is `L0208`.
- `deftest` defines the test's function under an introduced name, `test-` and the written name, so no name of the program collides with it, `rslisp check` and `rslisp types` leave it out, and its Rust name yields to an item the program wrote. A test name is not a binding, so `(doc pads-left)` finds only a binding the program wrote.
- Every command expands and type checks every test. A test emits Rust only under `rslisp test`, because the emitter writes only what `main` reaches.
- A second test of one name in one module is `L0333`, at the second test, with the first marked.
- A test's full name is its module's path, `/`, and its name, such as `text/format/pads-left`.

`assert` takes one form and an optional message. A failed `assert` ends its test. The message runs only when the check fails, as with Rust's `assert!`, so a message that panics does not fail a passing `assert`. A string literal is the closure's value, and any other message is `(str message)` in the closure, so a variable keeps its value. When the form is a call of the prelude's `=` with two arguments, `assert` passes each side to the check, in order, with the comparison as a closure, `(fn (left right) (= left right))`. So each side runs once, a side keeps its value, the comparison type checks as a written `=` does, with a literal or `none` taking its type from the other side, and a side without `show` is reported at that side. A mismatch between the two sides labels the whole `assert`, because the closure's names take its location. A failure shows both sides in their printed form. A side that is a `deref`, written or from a macro's expansion, is a Copy value, because `=` rejects any other, and a written `deref` turns off the borrow at a call, so `assert` passes that side as `(do side)`, whose value the call borrows. Each side still runs left then right. `assert` accepts what `=` accepts and rejects what `=` rejects, and a Copy side without `show` is reported at the `assert`, once. Any other form shows as written.

`assert` works in any code, as Rust's `assert!` does, so a helper that a test and `main` share builds, and a `(use prelude/test assert)` works under `build`, `run`, `eval` and the prompt. A failed `assert` panics, and with no runner the panic prints its location, its written form, both sides of `=` and its message:

```text
assertion failed at text/format.rslisp:12:3: (assert (= (pad-left "7" 3) "007"))
left:  "  7"
right: "007"
```

| The `assert` | Calls | Type |
| --- | --- | --- |
| any form other than `=` | `check-true` | `(fn ((ref str) (ref str) bool) unit)` |
| any form other than `=`, with a message | `check-true-with` | `(where ((m display)) (fn ((ref str) (ref str) bool (fn-once () m)) unit))` |
| `(= left right)` | `check-equal` | `(where ((a show) (b show)) (fn ((ref str) (ref str) (ref a) (ref b) (fn ((ref a) (ref b)) bool)) unit))` |
| `(= left right)`, with a message | `check-equal-with` | `(where ((a show) (b show) (m display)) (fn ((ref str) (ref str) (ref a) (ref b) (fn ((ref a) (ref b)) bool) (fn-once () m)) unit))` |

Each check takes the `assert`'s location, from `form-location`, and its written text first. A failed check stores its record in a static of the runtime, where the runner finds it, then panics.

`(run-tests :marker "<marker>")` expands to the program's test runner: one call per test of every module that `(program-modules)` returns, in load order and then source order, between a start and a finish. `:untested <module>` adds the list of names that `<module>` exports and no test writes. The checks and the runner are `rust` templates over runtime parts in `prelude/runtime/`, one per marker, so the emitted Rust holds a function only when it calls that function. The runner runs each test under `std::panic::catch_unwind` and writes the records that `rslisp test` reads.

**`rslisp test <path> [filter]` builds one program that runs the tests, and runs it.**

- A file is an entry: its module and every module it reaches through `use` run their tests. A directory makes every `.rslisp` file under it an entry, with the directory as the root of `use`.
- The program's `main` is `(run-tests :marker "<marker>")`, resolved in the scope of `prelude/test`, which `rslisp test` loads, and expanded after every module. An entry's own `main` is checked and does not run, and a test that calls it calls `main_2` in Rust.
- `filter` runs the tests whose full name contains it, and `--exact <name>` runs the test of that full name. `rslisp test` passes either to the program as its arguments, and the runner applies it. When no test is selected, a filter adds ``note: no test name contains `<filter>` `` and `--exact` adds ``note: no test is named `<name>` `` on standard error.
- `--untested <module>` adds `:untested <module>` to the call. A module that the program does not load is a usage error before the build, ``error: the program does not load module `<module>` ``.
- Tests run one at a time, in the order `run-tests` calls them, and read the standard input of `rslisp test`. The program's standard error passes through.

**The runner writes a record for each event: the marker, a printed form in the printed form of [RFC 0048](../rfcs/0048-printing-values.md), and a newline.** `rslisp test` finds a record anywhere in the program's standard output, as `eval` finds its marker, strips it, and passes the rest of the output through where the program wrote it.

| Record | Written |
| --- | --- |
| `(untested "<module>" <count of exports> "<name> <name> ...")` | first, only with `:untested` |
| `(start <count of selected tests>)` | before the first test |
| `(test "<full name>" "<docstring's first line>")` | before each selected test, followed by a flush |
| `(ok)` | after a test that returns |
| `(fail "<location>" "<written assert>")`, with an optional `"<message>"` | after a failed `assert` on a form other than `=` |
| `(fail-equal "<location>" "<written assert>" "<left>" "<right>")`, with an optional `"<message>"` | after a failed `assert` on `=`, with each side's printed form |
| `(panic "<location of the deftest>" "<message>")` | after any other panic |
| `(done)` | after the last test |

**`rslisp test` prints every line of the report from the records:**

```text
untested in text/format (1 of 3): trim-right
running 2 tests
test text/format/pads-left ... ok
test text/format/trims-right ... FAIL
FAIL text/format/trims-right: Removes trailing spaces, not inner ones. (text/format.rslisp:20:3)
    (assert (= (trim-right "a b  ") "a b "))
    left:  "a b"
    right: "a b "
1 passed, 1 failed
```

- A test's own output follows `test <name> ... ` on the same line, and the result starts the next line. `rslisp test` adds a newline before the result when that output does not end with one.
- A failure prints the test's full name, the first line of its docstring after a colon, and the location of the failed `assert`, or of the `deftest` for any other panic. The written `assert`, both sides of `=`, the message and a panic's message follow, each indented 4 columns.
- The untested line prints no names when every export is written: `untested in text/format (0 of 3)`.

**A test that ends the process, through `exit`, an abort or a stack overflow, crashes the run.** Its record of `(test ...)` has no result after it and the program writes no `(done)`. `rslisp test` then prints `CRASH` as the test's result, the reason, the number of tests not run when it is not 0, and a count line that names `crashed` and `not run`:

```text
running 3 tests
test crash/first-one ... ok
test crash/crashes ... CRASH
CRASH crash/crashes: the program exited with code 3
1 test not run
1 passed, 0 failed, 1 crashed, 1 not run
```

| The test | The reason |
| --- | --- |
| calls `(exit N)` | `the program exited with code N`, even for `(exit 0)` |
| aborts, or overflows its stack | `the program stopped on signal 6 (SIGABRT)`, after Rust's own lines on standard error for a stack overflow |
| is stopped by signal N | `the program stopped on signal N (<name>)`, with the name when it is the same on Linux and macOS |

| Exit code | When |
| --- | --- |
| 0 | every selected test passes, or no test is selected |
| 1 | a test fails or crashes, whatever the program's own exit code or signal, or the program has errors |
| 2 | a usage error |

A non-empty untested list does not change the exit code. `prelude/tests/` holds the prelude's own tests, one module per prelude module, outside the embedded prelude, and a test in `tests/cli.rs` runs `rslisp test prelude/tests --untested prelude` and records the report.

## Rust interop

`rust` binds a name to a piece of Rust:

```lisp
(rust-trait number "crate::rt::Number" i8 i16 i32 i64 u8 u16 u32 u64 f32 f64)
(def add (rust "{0} + {1}" (where ((n number)) (fn (n n) n))))
(def not (rust "!{0}" (fn (bool) bool)))
```

A `rust` template names a type parameter of its `where` by name, as `{target}` in `"{0}.parse::<{target}>()"`. The emitter fills it with the Rust type that the binder takes at that call. The name must be a binder of the template's `where`; a name with no matching binder is an error.

A `rust` form may list clauses after its type, as a `rust-type` does. A clause is a type and a template, and a call uses the first clause whose type equals the type that the `where`'s one binder takes at that call. Every other call, such as a call from a function generic over the binder's trait, uses the template after the string. A `rust` with clauses binds exactly one type parameter, `L0246`. A clause changes only the Rust text: each placeholder passes its argument as the template passes it, with the same borrow, receiver, index, operator or call, and the clause calls a function exactly when the template does, `L0247`, so every check before emission reads the template alone. A template with clauses is bound by a `def`, because only a `def`'s calls give the binder's type; an inline one is `L0603`.

```lisp
(def slurp
  (rust "crate::rt::Source::slurp({0})"
        (where ((s source)) (fn ((ref s)) (result string io-error)))
        (str "std::fs::read_to_string({0})")
        (string "std::fs::read_to_string({0})")
        (stdin "std::io::Write::flush(&mut std::io::stdout()).and_then(|()| std::io::read_to_string({0}))")))
```

Clauses belong to the `rust` kernel form because a macro sees forms and not types, so only the compiler can choose a template by the type that a binder takes at a call. A runtime trait chooses by type too, but its call and its implementations stay in every program that calls it.

`rust-type` binds a type name to a Rust type. `{0}` in the template is the first type parameter in the head, as `{0}` in a `rust` template is the first argument. A parameter that may be unsized is written `(name unsized)`, as `(rust-type (box (item unsized)) "Box<{0}>")` does. The mark says that the type accepts its argument behind a pointer, so a field's view in that position keeps its view instead of becoming a borrow. A program's own `Rc` or `Arc` type marks its parameter the same way that `box` does. A `rust-type` may also list clauses after its template. A clause is a type and a template, and it applies when the type's first argument equals the clause's type. Every other argument uses the template after the name. A `rust-type` with clauses takes exactly one type parameter.

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

A `rust-trait` may hold a `structural` clause: keyword and string pairs that state, as Rust text, the impl of the trait for a struct or an enum. A struct fills `:struct`, and each of its fields fills `:field`. A variant with a payload fills `:variant`, and each payload fills `:part`, as does each field of a struct payload, because a construction takes those fields by position. A variant without a payload fills `:bare`. The placeholders are:

| Placeholder | Meaning | Keys |
|---|---|---|
| `{0}`, `{1}`, ... | the method's parameters, the receiver first | `:struct`, `:variant`, `:bare` |
| `{name}` | the struct's name, or the variant's qualified name such as `tree/node`, escaped for a Rust string literal | `:struct`, `:variant`, `:bare` |
| `{parts}` | each field filled into `:field`, or each payload filled into `:part`, in declared order | `:struct`, `:variant` |
| `{field}` | the field's name | `:field` |
| `{value}` | a borrow of the field or payload. The `Box` a recursive enum adds is borrowed through, because the source never writes it | `:field`, `:part` |

`{{` and `}}` write a literal brace. The clause needs exactly one `method`, whose first parameter is `(ref self)`, because the impl has one body. A clause on a trait that `derive` writes, such as `std::clone::Clone`, or on a path another `rust-trait` binds is an error, because a type would get two impls of one Rust trait. A missing, unknown or repeated key, a key without a string, and a placeholder its key cannot fill are errors too, all `L0248`. The `structural` clause is told from an implementor as `(help "text")` is, because no type holds a string.

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
| Placeholders | `{0}`, `{1}` and so on take the call's arguments. `{&0}` takes an argument that Rust borrows by itself, as a format macro or a comparison operator does. No `&` is written before it, and the call does not move it. A written `ref` passed to it, where its type is not already a borrow, is the borrow Rust makes, so `(> (ref x) 3)` and `(> 3 (ref x))` emit `x > 3` and `3 > x`. Such a `ref` of a field borrows the field itself, not the borrow that reading the field gives, so `(= (ref (:model leaf)) (ref model))` emits `leaf.model == model`. A borrow of a value that is not Copy, passed at such a position where the other arguments pass the value itself, is dereferenced, because Rust's borrow of the operand moves nothing: `(= (ref model) r)` with `r : (ref string)` emits `model == *r`. A function that consumes its iterator, such as `count` or `reduce`, uses `{0}` and leaves the binding as it is. `{owned 0}` takes an iterator that a Rust method advances through `&mut self`, as `find`, `any?` and `every?` do. The call moves it, as `{0}` does, so a later use is a use after move. An argument that emits as a plain name, such as a local, a parameter or a capture, gets `.into_iter()` after it, so `(find values matches?)` emits `values.into_iter().find(...)` without making `values` `mut`. Any other argument, such as `(iter years)`, emits as `{0}` does: `years.iter().find(...)`. A template that is only `{owned N}` emits the argument as it is. `{target}` is the Rust type that the `where` binder `target` takes at the call. `{!name}` is a name that the template's text binds: every occurrence takes the same fresh Rust name, chosen not to hide a name at the call. `{{` and `}}` are literal braces. |
| Inlining | A `def` of a `rust` form emits nothing. Each use inlines the template. |
| Parentheses | A non-atomic argument is parenthesized unless its placeholder is the whole template, sits between `(` or `,` and `,` or `)`, or Rust's precedence and associativity keep its grouping on the side it takes. A prefixed argument such as `*x` is parenthesized only before `.`, `[`, `(` or `?`. |
| Deref place | A `*` written immediately before a placeholder is dropped when the call borrows a `mut` place, because the `*` and the borrow cancel. |
| Evaluation order | A repeated or out-of-order placeholder binds its arguments to temporaries first. |
| Values | A function template used as a value becomes a closure whose parameters carry the types its signature declares. A bare path is used as the path. |
| Runtime | `prelude/runtime/` holds the parts of `mod rt`. A program carries only the parts its code names, plus the signal part when its code writes to stdout, a check that reads the emitted text, so a string literal that holds `println!(` also brings the signal part in. Its `Number` trait carries only the literal conversions it calls. |
| Closed stdout | A program stops by SIGPIPE when its stdout closes, as a Unix filter does, instead of panicking. Rust ignores SIGPIPE at start-up, and stable rustc has no flag to keep it, so a program that writes to stdout starts `main` with the runtime's `restore_default_sigpipe`. A program that never prints carries no start-up call. |
| Unsized | A type parameter used only behind references is declared `?Sized` exactly when a reached instantiation is unsized, so a caller that passes `str` can fill it. |

### Formatting

The emitted Rust is formatted by `rustfmt` when `rslisp emit`, `rslisp build` or `rslisp run` writes it, not by a compiler stage. `rustfmt` reads the nearest `rustfmt.toml`, so the program's directory chooses the layout. If `rustfmt` is missing or fails, or runs past `EmittedFormatting::TIME_LIMIT` and the command stops it, the command prints one note and writes the unformatted Rust.

## Source formatting

`rslisp fmt <path>...` lays out each file in place, and each `.rslisp` file under a directory, and prints nothing when it succeeds. `rslisp fmt --check <path>...` writes nothing, prints the path of each file that would change, and exits 1 if any would. `rslisp fmt -` reads standard input and writes the laid-out text to standard output, and `--stdin-path <file>` names the file that the text comes from. The command exits 0 when every file is laid out, 1 when a file does not read or would change under `--check`, and 2 for a usage error or a file it cannot read or write. The layout has no options: 2 columns of indentation within an 80-column width.

The formatter owns every line break. A list stays on one line when it fits in the width and either is at most 40 columns long, or is at most 60 columns long and holds at most two lists at any depth inside it, except that a form whose header shape carries `:docstring`, a definer such as `defn`, stays on one line only when its body is one form. A docstring counts as a body form, so a documented definition always puts its docstring on its own line. A list that breaks lays out by the head's header shape (see Header shapes). A shaped list keeps its header on its first line and indents each later form 2 columns. A `use` keeps its module path and first name on its first line, and an `export` keeps its first name there; both then fill each line with names up to the 60-column fill width, and each later line starts under the first name. Any other call aligns its arguments under the first argument, or puts every argument on its own line 2 columns in when that takes fewer lines or the aligned layout does not fit, except that an atom too long to fit, with the parentheses that close after it, even 2 columns past its list's `(` counts as fitting when the formatter compares the layouts, and a list whose head is not a symbol lines up its forms under its head. A keyword and the form after it share a line. In a list that breaks, a run of two or more atoms in a row fills lines up to the fill width in the same way, unless the head's shape carries `:one-per-line`. A list, a keyword pair, a docstring and a multi-line string each end a run. The formatter sorts the names of each `use`, after its module path, and of each `export` in byte order of their text, and keeps the forms themselves in their written order. A `use` or `export` whose names carry a comment, or a blank line between them, keeps its written order. [RFC 0053](../rfcs/0053-source-formatter.md#recommendation) states the rules with examples.

The formatter reads shapes without expanding, so a file that reads but does not compile still lays out. It reads the `header-shape` forms at the top level of the prelude, of the file, and of every module that the file reaches through `use`, following `use` transitively through each module it reads, and finds each module's file against the file's directory and then each parent directory. `header-shape` is the one prelude form that the compiler names: the formatter reads files without expanding them, so it recognizes a shape statement by the name of the form that states it ([RFC 0053](../rfcs/0053-source-formatter.md#decision), decided on 2026-11-04). Its markers `:docstring` and `:one-per-line` are the only keywords the formatter names, for the same reason. A shape names its form by name, so a local that shadows a shaped name lays out as that form. A form whose shape it cannot find lays out like a call.

A comment on its own line takes the indentation of the form after it, and a trailing comment stays one space after its form, with any `)` that would follow it moved before it. A `)` takes a line of its own only after a comment on its own line that ends a list, and a comment right after `(` puts every form of its list on its own line. One blank line separates top-level forms, except that consecutive one-line `use` forms stay together, and so do consecutive one-line `export` forms. Inside a list, a blank line between two forms stays, at most one in a row. A string and a number keep their text, except that a docstring's later lines move by as many columns as its first line moves.

Before it writes a file, the formatter checks that the laid-out text reads to the same forms, comparing docstrings after removing the indentation their later lines share, and that laying it out again changes nothing. A failed check is an internal error that names the file, and the file stays as it was.

## Names

Every prelude name follows five rules ([RFC 0046](../rfcs/0046-names.md)):

1. A name is kebab-case.
2. A predicate ends with `?`.
3. A function that changes an argument ends with `!`. A `!` name next to a plain name is the plain name's in-place form, as `reverse!` is to `reverse`.
4. A prelude type constructs by its own name, as `vec` and `pair` do. A `defstruct` type constructs by `->name`, as `->car` does.
5. One concept has one name.

These names break a rule:

- `(str ...)` builds a `string`, an exception to rule 4.
- `then-some` and `then-ok` break rule 5 until one `then` replaces them, which needs type-checker work that [issue #280](https://github.com/KarlHarnois/rslisp/issues/280) tracks.

## Names in Rust

| rslisp | Rust |
|---|---|
| `make-point` | `make_point` |
| type `point` | `Point` |
| `admin?` | `is_admin` |
| `add-item!` | `add_item` |
| `point->string` | `point_to_string` |
| constant `admin?` | `IS_ADMIN` |
| other punctuation | spelled out, so `+` becomes `plus` and `\` becomes `backslash` |
| a name made only of `-` and `_` | each character spelled out, so `-` becomes `minus` and `_` becomes `underscore` |
| a local named `_` | `_`, Rust's wildcard, so a read of it is an error, `L0245` |
| a character that Rust does not allow in an identifier | its code point, so `☃` becomes `u2603` |
| a Rust keyword | a raw identifier, so `type` becomes `r#type` |
| a program type or trait that hides a Rust prelude name | suffixed with `2`, so `sized` becomes `Sized2` |
| a local whose name a binding still in scope uses | numbered, so `n` becomes `n_2` |
| an item a macro introduced whose name a written item takes | the next free number, so the positional function of `car` becomes `make_car_2` beside a program's own `make-car`, and a struct or enum takes the number with no underscore, as `Point2` |

Every symbol the reader accepts that names an item becomes a valid Rust identifier. Two items the program wrote that become one Rust name in one module are an error at the second item, so `a-b` beside `a_b` is reported, never left to rustc.

A program type or trait that mangles to a name Rust's 2021 prelude brings into
scope takes the suffix `2`, so the emitted name never hides `Sized`, `Clone`,
`Iterator`, `Vec` or another prelude name. A `rust-type` or `rust-trait` path is
never renamed.

An item a macro introduced yields its Rust name to an item the program wrote, whatever the order of their uses. The introduced item takes the next number that no other item of its module takes. `rslisp check` and `rslisp types` print an introduced function under the name of the macro whose value position expands to it, so the positional function of `car` prints as `->car`. They leave out an introduced function that no such macro names.

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

Unless a program writes ownership, traits, bounds or loops, the compiler infers them, and a written form is exact. A `type` signature is exact: a type parameter it declares without `copy` gets no inferred `copy` bound. A `derive` form lists a type's traits exactly. A written `ref` or `mut` turns off adjustment for that value, and so does a written `deref` whose result is not a borrow.

### 8.1 References at call sites

A value meets an expected type in four places:

- an argument
- a return value
- an annotation
- a function passed as a value

At each of them, the compiler adds one borrow, removes borrows, or copies a Copy value out of a borrow. It never clones. It decides each adjustment after inference, when every type is known.

- The rule applies to the outer reference only. `(vec (ref person))` never becomes `(vec person)`.
- A `(ref (vec a))` value meets a `(ref (slice a))` type as Rust's deref coercion does. The compiler writes no new syntax. A `(mut (vec a))` value meets a `(mut (slice a))` type the same way.
- A vector parameter takes the slice when no use needs the vector, as Clippy's `ptr_arg` asks of hand-written Rust. A shared vector parameter, inferred or written as `(ref (vec a))`, takes `(ref (slice a))`, and a mutable one, inferred or written as `(mut (vec a))`, takes `(mut (slice a))`. A written reference keeps its kind and its lifetime: only its referent shrinks. A use that passes it to a parameter that keeps its vector, makes a written `mut` borrow, owns the value, calls a vector-only function such as `push!`, or uses it in any other position keeps the vector. `set!` on an element changes a vector through its slice, so `(set! (at years 0) year)` lets `years` take `(mut (slice i64))`. `set!` on the whole parameter replaces the vector, so `(set! years (vec 2020))` keeps `(mut (vec i64))`. `iter` reads a vector through its slice, so `(for-each (iter cars) show)` lets `cars` take `(ref (slice car))`. A trait method's vector parameter takes the slice in the trait and in every impl only when no impl and no default body keeps the vector, so the declaration and every impl agree, and a method passed as a value keeps the vector. An inferred `string` parameter takes `str` when no use needs the owned string.
- A borrowed value whose type inference has not fixed never becomes `str` or a slice, which no variable or vector can hold. Where it meets a borrow of one, it becomes that borrow, so `(vec "Leaf" "Ariya")` passed to an unannotated function that compares each item with `"Leaf"` takes `&[&str]`.
- In a generic slot, a borrowed Copy value of 16 bytes or less is copied. A larger value stays borrowed. Parameter modes use the same limit.
- An unwritten parameter compared with a borrowed Copy item of 16 bytes or less takes the value, as `floor` in `(defn recent (years floor) (count (filter (iter years) (fn (y) (> (deref y) floor)))))` takes `i64` when `years` holds `i64`. A function used at one type is checked again at that type, so the rule holds when only the call fixes the item type. A parameter that the second check leaves open takes the pinned item type, or the pinned borrow when the body borrows it, so the function stays concrete. A larger or unknown item keeps the parameter borrowed.
- A written `ref` or `mut` turns the rule off for that value, and so does a written `deref` whose result is not a borrow. The compiler reports a mismatch as written.
- A `mut` borrow always takes the original place, never a copy or a temporary.
- If a function passed as a value needs an adjustment of its parameters, the compiler wraps it in a closure.
- If a body copies a generic parameter, the compiler infers the parameter's `copy` bound.
- If the name of a function that changes an argument does not end with `!`, the compiler warns. The emitted Rust drops the `!`.
- `deref` copies a Copy value out of a borrow. A `deref` whose result is still a borrow, as `(deref y)` on a `(ref (ref i64))` is, removes its one borrow and leaves the rest to the rule. A `deref` whose result type is still unknown at the call, as in a closure bound by `let` before its use, waits for that type, so `(let recent? (fn (y) (> (deref y) 2020)))` passed to `filter` emits `|y: &&i64| **y > 2020`.
- A command prints each function's inferred signature, so that a `type` form can pin it.

Rust adjusts a receiver's references automatically, but rslisp has no receivers, so the rule covers every call. A Copy value of 16 bytes or less travels in registers, so copying it costs no more than borrowing it.

### 8.2 Struct traits

Every struct implements each of `clone`, `partial-eq`, `eq`, `hash` and `show` that all its field types implement. `show` is inferred through its `structural` clause. If a struct's fields are all Copy and the program never changes the struct in place, the struct is `copy`. The program changes a struct in place when a `mut` parameter receives it or one of its fields changes.

- `(derive name trait...)` lists a struct's traits exactly and turns inference off for that struct.
- A written list can add `partial-ord`, `ord` or `default`, drop an inferred trait, or pin the set.
- A listed trait brings the traits Rust requires of it. `ord` brings `partial-ord`, `eq` and `partial-eq`.
- If a field blocks a listed trait, the `derive` form is an error.
- The compiler reports a missing trait at its use and names the field or `derive` form that blocks it.
- `prn` and `pr-str` print a value's printed form through `show`, which a struct implements when its field types do. `println` keeps printing user-facing text.
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

An optional `(rust "Path")` as the first form after the name makes the enum name a Rust type, such as `(enum (option value) (rust "Option" bare) (some value) none)`. The clause holds one string, a Rust path, or a type template when the string holds a placeholder such as `{0}`. An optional word `bare` after a path says the variants are in Rust's prelude. Only in that position does `(rust "...")` mean the clause. The compiler appends the enum's type parameters to a path, so `(option i64)` emits `Option<i64>`. Such an enum emits no Rust definition. A construction or pattern emits the path, then `::`, then the variant's Rust name, as `(option/some x)` emits `Option::Some(x)` and the pattern `none` emits `Option::None`. A `bare` enum emits the variant's Rust name alone, as `(option/some x)` emits `Some(x)` and the pattern `none` emits `None`. The compiler reads `bare` from the clause; it does not name Rust's prelude types or variants itself. The enum reads as a `rust-type` in its emitted Rust, its traits, its size, its display in diagnostics and `types`, and `try`. Its traits come from `rust-trait` entries only, with no inferred traits and no `derive`; a `derive` of it is an error, `L0435`.

A template clause fills its placeholders from the enum's type arguments and from the variant's payload. Such an enum has exactly one variant, with one positional payload part per placeholder; otherwise the compiler reports `L0438`. The type emits the template filled with the type arguments, so `(pair usize string)` emits `(usize, String)`. A construction and a pattern emit the template filled with the payload in order, so `(pair i x)` emits `(i, x)` and the pattern `(pair index line)` emits `(index, line)`.

```lisp
(enum shape
  (circle f64)
  (rect f64 f64)
  (labeled (struct (text string) (size f64)))
  point)
```

A construction is qualified. `(shape/circle 1.0)` constructs a payload variant, and `shape/point` is the value of a variant with no payload. A variant with a struct payload takes its fields positionally, in field order: `(shape/labeled (str "box") 2.0)`. Using a payload constructor as a value is an error. `(export shape)` exports the enum and every constructor it defines. The prelude defines `ok` and `err` as macros over `result/ok` and `result/err`, and `some` as a macro over `option/some`, so a program writes `(ok value)`, `(err error)` and `(some value)`. `none` is the `option/none` value.

An enum gets the traits of a struct under 8.2, with its payloads in place of fields. Each structural trait that all payloads support is inferred, and `copy` is inferred when nothing changes the enum in place. A function type in a payload type, directly or inside another type such as `(vec (fn () i64))`, allows `clone` and `copy`, and blocks comparison, hashing and `show`, because Rust compares function pointers unreliably and a function has no printed form. A written `derive` is exact, and an order follows the declared variant order. A blocked trait names the variant whose payload blocks it.

A payload that holds its own enum by value, directly or through another type that stores it by value, is stored in a `Box`. The compiler boxes the payload position, writes `Box::new` at construction, and a pattern binds the payload as if it were unboxed. A payload whose cycle passes through a `rust-type`, such as `(vec type)`, is not boxed: the compiler does not look through a Rust type. A written `(box type)` is the exact form and stops inference for that position. A value cycle adds one box.

`types` prints an `(enum ...)` line with the inferred boxes for each enum that has one.

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

A `rust-trait` that declares a method can also be implemented, and the compiler emits the Rust `impl`. The method's name and signature come from the `rust-trait` entry, so the program writes only the method body. Its implementor must be a struct or enum of the program, because Rust's coherence rule rejects an implementation for a type outside the crate. A `rust-trait` entry may also state a `(help "text")` line, which the compiler shows when a type does not implement the trait. Two written impls of one trait whose implementors unify, such as `(display (tree i64) ...)` beside `(display (tree a) ...)`, or a written impl beside a `derive` that lists the trait, are `L0459`, so rustc's `E0119` never reaches the user.

The prelude's `display` trait declares `fmt`, and the prelude's `display` macro writes the implementation from printing parts:

```lisp
(struct point (x i64) (y i64))
(display point (p) "(" (field p x) ", " (field p y) ")")
```

The parts follow `println`: a string is literal text, any other form prints with `display`, and `(str ...)` splices. A `(pr-str ...)` part prints its value's printed form. When the parts are exactly one `match` form, each arm body becomes a write of that body's parts, and a `(str ...)` arm writes those parts. Any other arm body is one part, and the rule applies again to a nested `match` or `if`. A single `if` form writes in both branches the same way. The type then works with `print`, `println`, `str`, and as the error type of a `main` that returns a `result`. An implementor that is not a struct or enum of the program is an error, `L0445`.

```lisp
(trait shape
  (area (fn ((ref self)) f64))
  (scale! (fn ((mut self) f64) unit)))

(impl shape circle
  (area (c) (* 3.0 (field c radius) (field c radius)))
  (scale! (c factor) (do)))
```

`self` is a type variable bound by the trait. A method is an ordinary function type whose first parameter states the receiver's ownership: `self`, `(ref self)` or `(mut self)`. Such a method emits a Rust receiver, `fn area(&self) -> f64`, and a call emits `c.area()`. Every other method has no receiver and emits an associated function, `Shape::name(...)`. A call picks the implementor from the type of a parameter or the result that is `self`, so a method whose signature never mentions `self` is an error at each call and each method value, `L0456`. A signature that mentions `self` only through an associated type, as `(fn (item) i64)` does, never mentions `self` either. A method whose signature holds `self` inside another type, as `(vec self)` does, emits `where Self: Sized` on that method, not on the trait, because a Rust container holds only sized values. A default method whose signature holds `self` by value, as `(twin (fn (self) self) self)` does, emits it too, because its body holds a value of the implementor. A method value such as `(map shapes area)` emits a path, `Shape::area`. A method with a body after its type has a default. An impl may leave that method out, and the body emits inside the Rust trait item; an impl that writes the method overrides the default. A trait head that is a list holds the trait's name and then its supertraits: `(trait (shape (display) (clone)) ...)` emits `trait Shape: std::fmt::Display + Clone`. Every implementor must implement each supertrait, and the compiler reports a missing one at the impl. A default method and a supertrait need the compiler because a macro cannot add a body or a bound to a Rust trait item.

An `impl` writes each method like `defn`. Its parameter types come from the trait, and the first parameter emits as Rust's `self`. A trait signature owns its lifetimes: the compiler adds the shared lifetime there, and an impl copies the parameter types exactly, except that a method without a receiver narrows the shared lifetime to the parameters whose borrows reach its result. An impl for a type that holds a borrow writes the type with its lifetime, as `impl<'a> Finder for Snippet<'a>`, and its methods elide every lifetime Rust elides, so `(badge (fn (i64 (ref self)) (ref str)))` emits `fn badge(year: i64, c: &Self) -> &str` there. Its methods write `Self` in a parameter or a result wherever the trait writes `self` there, so `(twin (fn (self) self))` emits `fn twin(self) -> Self`, `(fn (self) (vec self))` emits `-> Vec<Self>` and `(newer (fn (self self) self))` emits `fn newer(self, other: Self) -> Self`. A receiver taken by value lends no lifetime for Rust to elide in the result, and a parameter written as `Car<'_>` would take a lifetime of its own that a `Self` result cannot return. A missing method, an extra method and a method the trait does not declare are errors. An `impl` may name a trait and a struct or enum from any visible module. Every module of a program is one Rust crate, so Rust's coherence rule never rejects such an `impl`. A method name is a top-level name, so two traits in one module cannot share one; `use` picks between modules.

`(export shape)` exports the trait and every method it declares, as `(export shape)` exports an enum and its constructors. `(use shapes shape)` imports the trait and every method it declares. A method name that clashes with a name the module already binds is the existing duplicate-name error.

A trait emits in its own module's Rust `mod`, with the visibility the emitter gives an exported struct. An `impl` emits in the `mod` of the module that writes it. A module that calls a method with Rust method syntax has the trait in scope: the compiler emits a Rust `use` of the trait at the top of that module's `mod`, once per trait and only in a module that needs it. A method value emits the trait's path from the calling module, as `Shape::area` with the `use` or as a full path without one.

A method is an ordinary function: `(area c)` is a call, and `(map shapes area)` passes the method as a value. A call on an unknown type adds the trait's bound, as `<` adds `ord`, and a written signature stays exact.

In a type position, `(box shape)` where `shape` is a trait emits `Box<dyn Shape>`. A `(box value)` whose expected type is `(box shape)` emits the coercion to the trait object, and the only allocation is the `box` the source writes. A boxed value calls the trait's methods as any value does, as `(area shape)` does. A trait is boxable when every method takes `(ref self)` or `(mut self)`, no method is generic, no method returns `self` by value, and the trait declares no associated type. A trait that is not boxable is an error at the `(box ...)` type, `L0448`. A trait object needs the compiler because only the compiler knows a trait's implementors and emits the coercion.

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

A lifetime is declared with the type variables, as `(where ('a 'b) ...)` does. A reference names it as its first part, `(ref 'a str)` or `(mut 'a point)`. A written lifetime is exact, and the compiler adds no shared lifetime to a type that writes one. A lifetime that links positions, as `'a` links the first parameter and the result, stays named in the emitted Rust. One that appears once and nowhere in the result emits as Rust's elided form, so `b` in `first` above becomes `b: &str`. One inside an `iterator` item stays named even when it appears once, because Rust rejects an elided lifetime inside `impl Iterator`: `(where ('a) (fn ((iterator (ref 'a str))) usize))` emits `fn count_long<'a>(names: impl Iterator<Item = &'a str>) -> usize`.

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

A trait signature owns its lifetimes. The compiler adds the shared lifetime to the trait's declaration, and an impl copies the parameter types exactly, except that a method without a receiver narrows the shared lifetime to the parameters whose borrows reach its result. An impl for a type that holds a borrow writes that type with its lifetime, as `impl<'a> Finder for Snippet<'a>`, and its methods elide every lifetime Rust elides. A method with a `(ref self)` or `(mut self)` receiver takes no shared lifetime while only the receiver's borrow reaches its result, because Rust's receiver elision makes the result borrow from the receiver. When the trait's default body or any impl lets another borrowed argument reach the result, the result shares the lifetime with each parameter whose borrow reaches it, the receiver included, so `(label (fn ((ref self) (ref str)) (pair self (ref str))))` emits `fn label<'a>(&'a self, _: &'a str) -> (Self, &'a str) where Self: Sized`, and `(stamp! (fn ((mut self) (ref str)) (ref str)))`, whose receiver never reaches its result, emits `fn stamp<'a>(&mut self, _: &'a str) -> &'a str`. A trait method whose parameters hold no borrow, with `self` counted as holding none, gives each elided borrow of its result `'static` in the trait and in every impl, as a function does: `(badge (fn (i64 self) (ref str)))` emits `fn badge(_: i64, _: Self) -> &'static str`. A trait signature cannot name the lifetime that an implementor holds, so an impl for such a type is an error, `L0605`, when a borrowed argument that the trait does not write as `self` reaches a result whose only borrow is the implementor's, when the implementor's borrow reaches a `'static` result, or when a parameter that the trait writes as `self` by value lends its borrow to a result that the trait ties to the reference parameters while no parameter borrows `self`, as `(echo (c name) (:model c))` does for `(echo (fn (self (ref str)) (ref str)))`. A call lends `self`'s borrow only when the called function's result borrows from that argument, so `(echo (c name) (second c name))` runs when `second` returns only `name`. A call of a closure or a trait method counts as lending none, and rustc reports what it rejects. A `match` lends the borrow of the value it checks only through an arm that returns one of its pattern bindings, so `(match r ((car m) name) (bike name))` lends none. Rust accepts that impl only under a `where Self: 'a` bound, which every generic caller of the method would then have to prove, so the compiler reports the impl and suggests `(ref self)`, whose borrow the result may share. A parameter such as `(ref self)` beside it lets the result borrow from the by-value `self` too, because both hold the same implementor's lifetime.

A returned borrow of a local value is an error, `L0510`, because the local is dropped when the function returns. The check covers every function and closure. A parameter that owns its value is a local, so `(let first (fn (items) (at items 0)))` is the error, where it would emit `|items: Vec<i64>| &items[0]`. The result of a call, an `if`, a `match` or a block is a temporary that Rust drops when the function returns, unless every branch returns a parameter that the function borrows, so a borrow of it is a borrow of a local: a `match` arm `(none (at (vec 0) 0))` is the error, where it would emit `&vec![0_i64][0]`. Rust promotes a borrow of a constant expression to `'static` instead, so a borrow of a template over constants runs: `(ref (+ 1 2))` emits `&(1_i64 + 2_i64)`, and `(ref (pair "Leaf" 2019))` emits `&("Leaf", 2019_i64)`. A constant is a literal, a top-level `def`, a template over constants, a variant or a struct of constants, or a block whose last form is a constant. A template is a constant unless its text calls a function, a method or a macro, so a cast such as `"{0} as i64"`, a path such as `i64::MAX` and a constructor such as `Some({0})`, whose name starts with a capital letter, keep it a constant. So do the functions and the macros that Rust promotes, such as `size_of` and `stringify!`, whatever the macro's brackets hold. Rust promotes an operator only over numbers, booleans and characters, so `(ref (= "Leaf" "Ariya"))` is the error, and an integer `/` or `rem` only when the divisor, passed or written in the template, is a literal, a negated literal or a `def` other than `0`, with a constant dividend when the divisor is `-1`. A call to a program function is never a constant. A closure may return a borrow of a value that it captures. The check follows the returned value through blocks, `if`, `match`, struct and enum constructions, and calls whose result borrows the argument. rustc reports the lifetime errors the compiler does not see, mapped back to the source and the names the program wrote.

`ref`, `mut`, `where`, `struct` and `enum` gained a lifetime position or binder, so this milestone needs no kernel form. A macro cannot add a lifetime to a function's emitted signature or to a struct's generic list, because the compiler builds both from the inferred type rather than from a form a macro returns.
