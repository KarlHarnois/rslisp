# RFC 0038: Field keywords

**Status:** Accepted. Its construction call rule is superseded by 0039. Its field view rule is superseded by 0040.
**Date:** 2026-10-26

## Problem

A struct field is spelled four ways today. A declaration writes a group, `(year u16)`. A constructor call writes positions, `(make-car "Honda" "Civic" 2019 false)`. A read calls a generated accessor, `(car-year civic)`. An update writes a group again, `(assoc civic (year 2020))`. Each spelling is its own rule, the accessors clash when two structs share a field name, and a constructor cannot be called by field name.

A struct field that stores a string must also be written `(ref str)`. A bare `str` field passes `rslisp check` and emits `name: str`, which rustc rejects. A bare `str` enum payload fails the type stage, so no program can construct it.

## Options

### One spelling: a symbol that starts with `:` names a field

A keyword is a symbol whose name is `:` followed by at least one character. The reader does not change: `:year` is an ordinary symbol. Declaration, construction, reading and updating all name a field the same way:

```lisp
(defstruct car
  :make str
  :model str
  :year u16
  :electric? bool)

(defstruct user
  :name str
  :admin? bool
  (defaults :admin? false))

(def leaf (->car :make "Nissan" :model "Leaf" :year 2024 :electric? true))
(def civic (->car "Honda" "Civic" 2019 false))
(def build ->car)

(:year leaf)
(map (iter cars) :year)
(assoc civic :year 2020)
(->user :name "ana")

(defn greet (:name str :greeting (defaults :greeting "hello"))
  (str greeting ", " name))
```

```rust
struct Car<'a> {
    make: &'a str,
    model: &'a str,
    year: u16,
    is_electric: bool,
}

fn make_car<'a>(make: &'a str, model: &'a str, year: u16, is_electric: bool) -> Car<'a> {
    Car { make, model, year, is_electric }
}

let leaf = Car { make: "Nissan", model: "Leaf", year: 2024, is_electric: true };
let civic = make_car("Honda", "Civic", 2019, false);
let build = make_car;
let year = leaf.year;
let newest = cars.iter().map(|value: &Car| value.year).max();
let updated = Car { year: 2020, ..civic };
```

The parts of the design:

- **Declaration.** `defstruct` reads `:field type` pairs. A compound type is one form, so `:tags (vec str)` is one field. The kernel `struct` and `new` forms keep their `(field type)` groups.
- **Reading.** `(:year car)` expands to the kernel `field` form, and `:year` alone in value position expands to a closure that reads the field. `defstruct` generates no accessors. A keyword cannot be bound by `let`, `def`, `fn` parameters or patterns.
- **Construction.** `defstruct` generates one constructor, `->car`. A call whose arguments read as `:field value` pairs naming distinct fields, with every omitted field defaulted, is a call by name. Otherwise, a call with one argument per field is positional. Otherwise, the call is an error. A call by name expands to `new`, so each value takes its field's type. A positional call and `->car` in value position use a hidden positional function that no source can name, emitted as `make_car`. `make-car` is an ordinary free name.
- **Macros in value position.** A macro may answer for its own name in value position. A macro that does not keeps today's error.
- **Defaults.** One `(defaults :field value ...)` clause, recognized by its `defaults` head, comes last, in a place where a field keyword would go. A call by name may omit a defaulted field. A positional call takes every field.
- **Updates.** `(assoc value :field new ...)` updates fields. Any other key keeps today's collection update. The grouped form `(assoc value (field new))` becomes an error that shows the keyword form.
- **Keyword parameters.** A `defn` parameter list whose first element is a keyword declares keyword parameters, read like `defstruct` fields, with an optional type after each. The function is called by name, by position, or passed as a value, with the constructor's rule. In such a list, a list headed by `defaults` is always the defaults clause, even right after a keyword.
- **Visibility.** Exporting a struct makes every field public to the modules that import it. A struct that is not exported stays private to its module, fields included.
- **The marker.** The compiler states `:` once. The `code` module gives macros `keyword?` and `keyword-name`, so no prelude macro restates the marker.

### Views stored in fields are borrows

A bare `str` or `(slice a)` written as a struct field or an enum payload means a borrow, exactly as the written `(ref str)` or `(ref (slice a))` does. Rust cannot store an unsized view by value, so this is the only meaning the type can have. The owned forms stay `string` and `(vec a)`.

```lisp
(defstruct report :title str :scores (slice i64))
```

```rust
struct Report<'a> {
    title: &'a str,
    scores: &'a [i64],
}
```

The compiler recognizes a view by its resolved type, never by the source text, so a program type that shadows `str` is unaffected. When a struct that borrows a view must outlive the data it borrows, the error's help line suggests the owned type.

## Recommendation

Adopt both parts. One spelling for a field is fewer rules than four, and every part compiles to a struct literal, a field read, a function or a closure, so the emitted Rust stays hand-written Rust with no hidden clone or allocation. The compiler grows only where the prelude cannot reach: keyword reads, because a prelude macro per field would clash between two structs with the same field name, macros in value position, because a macro runs only at the head of a list, and the view rule, because it depends on resolved types.

## Decision

The project owner accepted the design on 2026-10-26, with these choices:

- **Defaults** are one `(defaults :field value ...)` clause, last, recognized by its head, in both `defstruct` and `defn`.
- **Views** in fields and payloads are borrows, for `str` and `slice` alike, with a help line that suggests the owned type.
- **The constructor** uses a hidden positional function behind the `->car` macro. A research round confirmed that a macro can define a macro, that hygiene hides the function from user code but not from `->car`, and that `->car` works from another module. A program's own `make-car` keeps the Rust name `make_car`, and the hidden function takes the next free name.
- **The compiler owns `:`**, and the `code` module gains `keyword?` and `keyword-name`.
- **Macros in value position** use trial expansion: the expander passes the macro its own bare symbol as the form. If the macro returns a form, the expander uses it. If the macro fails, the expander reports today's error.

## Rejected options and why

- **`:default (:field value)` for defaults:** it reserves `default` as a field name, and `(:field value)` reads as a field read.
- **A `defaults` clause found as the one list among the fields:** a compound field type such as `(vec str)` is also a list.
- **An error for a bare `slice` field:** `str` would borrow while `slice` errors, an exception beside the `str` rule.
- **`Box<[T]>` or `Vec<T>` for a bare `slice` field:** each allocates where the source wrote none, and `Vec` silently changes the type's meaning.
- **A closure in place of the hidden function:** a closure value cannot go where a function type is expected today.
- **Opting in to value position with a second macro parameter, `(macro (form position) ...)`:** it changes the shape of the `macro` kernel form and adds a rule, where trial expansion adds none.
- **A visible `make-car` beside `->car`:** two names for one constructor, and `make-car` stays taken.
- **A `:` marker stated in the prelude:** the compiler also needs the marker for keyword reads, so the fact would be stated twice.
- **Keywords as runtime values or map keys:** a keyword read always succeeds because the struct's fields are known when the code is written. A map key is a runtime value that may be missing, so maps keep their current syntax.

## Rule

Each build that lands a part of this design states its rule in [docs/language.md](../docs/language.md), in the sections on the `code` module, structs, field keywords, modules, value updates and `defn`.
