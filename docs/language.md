# The rslisp language

`rslisp` is a placeholder name. Source files use the `.rslisp` extension.

The language is a Lisp that compiles to Rust. Its design follows four principles:

1. **Few rules, no exceptions.** A feature that needs an exception gets redesigned or left out.
2. **Small compiler, big prelude.** Anything the language can express lives in the prelude, written in the language itself. The compiler only reads, checks, reports and emits.
3. **Rust's performance and Rust's model.** Ownership, borrowing, traits and generics work as in Rust. The compiler never adds a hidden clone or allocation.
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
| `fn` | `(fn (param...) body...)` | `fn` item or closure | params: binders. body: expressions. |
| `def` | `(def name value)` | `fn`, `const`, `static`, or nothing | name: binder. value: expression. |
| `let` | `(let name value)` | `let` | name: binder. value: expression. |
| `do` | `(do form...)` | block | forms: expressions. At module level: definitions. |
| `if` | `(if test then else)` | `if`/`else` | all: expressions. |
| `type` | `(type name type)` | signature | name: reference to a `def`. type: type. |
| `struct` | `(struct name (field type)...)` | `struct` | name: type binder. fields: field labels with types. |
| `new` | `(new name (field value)...)` | struct literal | name: type. fields: field labels with expressions. |
| `field` | `(field value name)` | `value.name` | value: expression. name: field label. |
| `quote` | `(quote form)` | none | form: data. |
| `macro` | `(def name (macro (form) body...))` | none | form: binder. body: expressions. |
| `rust` | `(rust "template" type twin)` | the inlined template | type: type. twin: optional expression. |
| `rust-trait` | `(rust-trait name "Path" type...)` | trait bound | name: type binder. types: types. |
| `use` | `(use module name...)` | path | module: module path. names: binders. |
| `export` | `(export name...)` | `pub` | names: references in every namespace. |

In type positions, the kernel provides `fn`, `ref`, `mut`, `where`, `_`, and the primitive types `bool char str string unit i8 i16 i32 i64 i128 isize u8 u16 u32 u64 u128 usize f32 f64`.

### Module bodies

A module body is expanded in two passes:

1. Each top-level form is expanded in order until its head is a definition form: `def`, `type`, `struct`, `rust-trait`, `use`, `export` or `do`. The names it defines are registered. A `def` whose value is a `macro` is evaluated right away.
2. `def` values, types and `export` lists are expanded.

Values and types are therefore order-independent, like Rust items. A macro must appear above its first top-level use.

### Bodies

A body is the sequence of forms in `fn`, `macro` and `do`. A `let` binds its name for the rest of the enclosing body. The value of a body is the value of its last form, or unit if the body is empty or ends with a `let`.

## Types

Type syntax mirrors value syntax:

| Type | Rust |
|---|---|
| `i64` | `i64` |
| `(fn (i64 i64) i64)` | `fn(i64, i64) -> i64` |
| `(ref str)` | `&str` |
| `(mut point)` | `&mut Point` |
| `(where ((n number)) (fn (n) n))` | `fn<N: Number>(N) -> N` |
| `_` | inferred |

Type variables are bound only by `where`. An unknown type name is an unbound-name error that suggests the closest name.

```lisp
(type add (fn (i64 i64) i64))
(defn add (x y) (+ x y))

(type identity (where (a) (fn (a) a)))
(defn identity (x) x)

(type scale (fn (f64 _) _))
```

Inference is Hindley-Milner with trait bounds. Top-level functions generalize to Rust generics. Local `let` bindings stay monomorphic, because Rust closures cannot be generic.

A type parameter satisfies a trait when its `where` names the trait, or when every type its traits allow implements the trait. `(n float)` therefore satisfies `number` and `display`. The emitted Rust names any such bound that the runtime traits' supertraits do not already give.

### Literals

- An integer literal can become any number type, so `(make-point 1 2)` emits `make_point(1.0, 2.0)`.
- A float literal can become `f32` or `f64`.
- Every literal is checked against the range of its type.
- A literal whose type is still open after inference defaults to `i64` or `f64`, and the emitted literal carries a suffix (`3_i64`).
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

## Ownership

Ownership is part of the type, as in Rust. `(ref t)` is `&T` and `(mut t)` is `&mut T`. Type inference ignores ownership except where it is written, and a separate pass decides how each parameter is passed.

- **Modes.** A parameter without a written type gets the cheapest mode its uses need: copy for Copy types, then ref, then mut, then own. A use through a call follows the called function's mode, and the modes are recomputed until nothing changes.
- **Written types.** A type written without `ref` or `mut`, in a `type` signature or a binder annotation, means owned, as in Rust.
- **Call sites.** An argument is borrowed automatically when the parameter is a reference. A method-style template such as `"{0}.clone()"` borrows its receiver the way Rust's method calls do.
- **Fields.** Reading a Copy field copies it. Reading any other field borrows it: `(field person name)` has type `(ref string)`.
- **No hidden copies.** Copying heap data is always written `(clone x)`, which needs the `clone` trait. Every struct implements it, because `struct` derives `Clone`. `(ref x)` and `(mut x)` force a borrow.
- **Moves.** The compiler reports a use after move, points at the move, and suggests `(clone x)`. A move in either branch of an `if` counts.
- **Borrowed results.** A function may return a borrow when it has one borrowed parameter, or none for a string literal. With more than one, it reports an error, because lifetimes cannot be written yet.
- **Closures.** A function-typed parameter is `impl Fn`. A closure returned from a function is a `move` closure. A closure that only borrows or copies its captures is Copy and can be passed more than once. One that moves a capture can be used once.
- **rustc.** Any lifetime or aliasing error that slips past these checks comes from rustc, mapped back to the source and the user's names.

## Macros

A macro is a function from form to form that runs at compile time:

```lisp
(def defn
  (macro (form)
    (list (quote def) (nth form 1)
          (prepend (quote fn) (drop 2 form)))))
```

Macro bodies run in the compiler's interpreter. They use the kernel forms, the built-in `code` module and any function whose definition has already been expanded. A `rust` definition can name a compile-time twin from `code`, so `+`, `<` and `true` work in macro bodies as they do at runtime.

The prelude builds three tools on top of `macro`:

| Tool | Example |
|---|---|
| `defmacro` | `(defmacro unless (test body ...) (template (if (unquote test) (do) (do (splice body)))))` |
| `template` | `(template (def (unquote name) (splice body)))` |
| `defrule` | `(defrule unless ((test body ...) (if test (do) (do body ...))))` |

A `defrule` clause is a pattern and a template. In a pattern, `name ...` matches zero or more forms, at any depth, and `(quote name)` matches the symbol `name` itself. A `defmacro` parameter list is a pattern in the same language.

### The `code` module

The compiler provides the `code` module for macro bodies. Code that uses it exists only at compile time.

| Built-ins | Purpose |
|---|---|
| `first rest list prepend concat length empty? nth take drop` | Build and take apart lists of forms. |
| `symbol? list? string? number?` | Test what kind of form a value is. |
| `symbol-name`, `(symbol context part...)`, `(string part...)` | Read and build names and strings. |
| `(same-binding? left right)` | Test whether two symbols refer to the same binding. |
| `(fail "message" form)` | Stop expansion with an error at `form`. |
| `number-add number-sub number-mul number-div number-rem number-lt number-le number-gt number-ge value-eq bool-not bool-true bool-false` | Compile-time twins of the prelude's Rust interop. |

### Hygiene

Macros are hygienic. The expander uses sets of scopes:

- Every symbol carries a set of scopes.
- A module, a `fn` and the rest of a body after a `let` each add a fresh scope to the forms inside them.
- Each macro call adds a fresh introduction scope to its input and flips it on its output, so only the symbols the macro introduced carry it.
- A macro call in a definition context also adds a use-site scope to its input. Binders that a definition introduces lose their use-site scopes.
- A reference resolves to the binding with the same name and namespace whose scope set is the largest subset of the reference's scopes.
- `quote` removes the scopes of enclosing `fn`, `let` and `where` forms from its datum. Quoted names in different helper functions can then refer to each other, and a macro's local variables never leak into the code it returns.

Hygiene gives three guarantees:

1. A binding that a macro introduces never captures a user's binding, and a user's binding never captures the macro's.
2. A free symbol in a macro's output means what it meant where the macro was defined.
3. A name that comes from the macro's input binds in the user's scope.

`(symbol context part...)` builds a symbol with the scopes of `context`. It is the one deliberate way to introduce a name the user can see:

```lisp
(defstruct point (x f64) (y f64))   ; defines make-point, point-x and point-y
```

Field names are not hygienic. They are looked up through the struct type.

## Modules

- One file is one module. `(use text/format pad-left)` loads `text/format.rslisp` relative to the entry file's directory and binds `pad-left`.
- A name is private to its file until an `(export ...)` lists it. `export` can list imported names, which re-exports them.
- Every user module imports the prelude. An explicit `use` shadows a prelude name.
- Every prelude module imports the kernel.
- Import cycles are an error.

## Rust interop

`rust` binds a name to a piece of Rust:

```lisp
(rust-trait number "crate::rt::Number" i8 i16 i32 i64 u8 u16 u32 u64 f32 f64)
(def add (rust "{0} + {1}" (where ((n number)) (fn (n n) n)) number-add))
(def true (rust "true" bool bool-true))
```

| Template rule | Effect |
|---|---|
| Placeholders | `{0}`, `{1}` and so on take the call's arguments. `{{` and `}}` are literal braces. |
| Inlining | A `def` of a `rust` form emits nothing. Each use inlines the template. |
| Parentheses | A non-atomic argument is parenthesized unless its placeholder is the whole template or sits between `(` or `,` and `,` or `)`. |
| Evaluation order | A repeated or out-of-order placeholder binds its arguments to temporaries first. |
| Values | A function template used as a value becomes a closure. A bare path is used as the path. |
| Runtime | `prelude/runtime.rs` is emitted as `mod rt` when a bound names `crate::rt::`. |
| Unsized | A type parameter used only behind a reference is declared `?Sized`, so `str` can fill it. |

## Names in Rust

| rslisp | Rust |
|---|---|
| `make-point` | `make_point` |
| type `point` | `Point` |
| `admin?` | `is_admin` |
| constant `admin?` | `IS_ADMIN` |
| other punctuation | spelled out, so `+` becomes `plus` |
| a Rust keyword | a raw identifier, so `type` becomes `r#type` |

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
