# RFC 0058: A scoped let and a flat local

**Status:** Accepted. Its statement that `let-pair` and `let-fields` keep their names, and their place in the migration, are superseded by 0059, which replaces them with `let-match`. Its statement that the build emits each value into the loop's own binding is amended on 2026-11-07: `loop` calls a `fn` of its names with its values as the arguments when no value names an earlier binder of its list, and binds the list with `let` before that call otherwise, so a `loop` whose values see no earlier name emits as before, and a sequential `loop` whose body holds `try`, which stays a closure, keeps a copy of each value.
**Date:** 2026-11-02

## Problem

`(let name value)` binds `name` for the rest of the enclosing body and takes no body of its own:

```lisp
(defn distance (a b)
  (let dx (- (:x a) (:x b)))
  (let dy (- (:y a) (:y b)))
  (sqrt (+ (* dx dx) (* dy dy))))
```

In Common Lisp, Scheme, Racket and Clojure, `let` encloses a body, and its bindings end at its closing parenthesis. A Lisper who reads `(let dx ...)` with no body sees a mistake. The language also disagrees with itself: `let-pair`, `let-fields` and `let-try` each take a body, so the `let-` prefix means "bind, then a body" everywhere except on `let`.

A flat binding does not show where it ends. `rslisp repl` runs each expression input in a generated `main`, so it accepts `(let total (+ 2 3))` and then loses `total` before the next input, with nothing on the screen to say so. A binding whose scope ends at its own closing parenthesis shows where it ends.

Today a program scopes a binding only through `do`:

```lisp
(do (let x 5) (let y 7) (+ y x))
```

## Options

Options A, B and C write a scoped binding in the form the project owner chose:

```lisp
(let (x 5
      y 7)
  (+ y x))
```

The list after `let` alternates names and values, as Clojure's binding vector does. The reader treats only `(`, `)`, `"`, `;` and whitespace specially, so the list uses parentheses where Clojure uses brackets. Every option that has this form proposes the same rules:

- **Bindings are sequential.** Each value sees the names bound before it, so `(let (x 5 y (+ x 1)) y)` is `6`. One form covers what Scheme splits into `let` and `let*`, and the order matches Rust's.
- **A later binding may shadow an earlier one,** as a flat binding may today.
- **An empty list is allowed.** `(let () body ...)` is `(do body ...)`, so a macro that builds a `let` from zero bindings needs no special case.
- **A name without a value is an error.** The label points at the last name with "this name has no value", and the help prints the user's list in the form it is written in, with the placeholder `value` after that name, as `(let (total (sum xs) count value) ...)`. When every item of the binding list is a two-item list, so the last one lacks a value, the binding list reads two ways: the old `((index 0))` binds `index` to `0`, and `((count usize))` is a typed binder that lacks its value. The help then shows both fixes: a value after that list, written as the placeholder `value` because the compiler does not know it, as `(loop ((count usize) value) ...)`, or the binding list in the new shape, with each `(name value)` written as `name value`, as `(loop (index 0) ...)`. The rule covers any old list with an odd number of bindings, such as `((index 0) (total 0) (rest items))`. A list in the new shape whose last typed binder lacks its value and that holds a bare name or any other item that is not a two-item list, such as `(count 0 (total i64))`, gets the plain help, because no old list holds such an item. Both fixes print the user's own list, as the `L0227` help prints the binding as written, so `(let-try ((text (slurp path))) ...)` gets `(let-try (text (slurp path)) ...)`. `let`, `let-try` and `loop` each report it with one new macro-stage code, `L0334`, because each pairs its own list before any `let` sees it.
- **A binder is what the flat form takes:** a name, or a name and its type, as in `(let ((year i64) (deref y) total 0) ...)`. A keyword in a binder position is an error, as it is today.
- **A `let` with no body is an error.** Its bindings reach no form, so the form is always a mistake. `let`, `let-try` and `loop` each report it for their own form, with a second new macro-stage code, `L0335`, because a `loop` expands to a `let` that has a body. The help for `let` names the flat form for a binding that should last to the end of the body, and the help for `let-try` and `loop` says to write the forms that use the bindings.
- **The body follows `do`'s rules.** Its value is its last form, and its last form is in tail position, so `recur` works there.

A prototype on `main` at `54057b9` defines the scoped form as a `defrule` over today's flat `let`, under the temporary name `scoped`:

```lisp
(defrule scoped
  ((() body ...) (do body ...))
  (((name value more ...) body ...) (do (let name value) (scoped (more ...) body ...))))
```

It needs no compiler change. Its recursion stops with `L0326` between 340 and 360 pairs, and a flat body has no such limit, so the prelude `let` expands every pair in one step and no expansion limit applies. A scoped form at the end of a function body emits flat `let` statements, and a scoped form in argument position emits a Rust block, as hand-written Rust does:

```lisp
(defn total ()
  (scoped (x 5
           y 7)
    (+ y x)))

(defn report (n)
  (println (scoped (doubled (* n 2)) doubled))
  (scoped (half (/ n 2)
           quarter (/ half 2))
    (println half)
    (println quarter)))
```

```rust
fn total() -> i64 {
    let x: i64 = 5;
    let y: i64 = 7;
    y + x
}

fn report(n: i64) {
    println!("{}", {
        let doubled: i64 = n * 2;
        doubled
    });
    let half: i64 = n / 2;
    let quarter: i64 = half / 2;
    println!("{}", half);
    println!("{}", quarter)
}
```

A name used after the scoped form is `L0201`, so the scope ends where the parenthesis does. A scoped form that shadows a name of the enclosing body emits a renamed binding in a block, as a nested `do` does today:

```lisp
(defn shadow ()
  (let x 1)
  (scoped (x 2) (println x))
  (println x))
```

```rust
fn shadow() {
    let x: i64 = 1;
    {
        let x_2: i64 = 2;
        println!("{}", x_2)
    }
    println!("{}", x)
}
```

Rust accepts the same name in an inner block, so the emitter could keep `x`. That is a separate change to how a nested block names its bindings, but a scoped `let` makes the case common.

### A: `local` for the flat form, and `let` scoped in the prelude

The kernel form `let` becomes `local`, with the same rules. `let` becomes a prelude macro that expands to `do` and `local`, as the prototype does. `let-pair`, `let-fields` and `let-try` keep their names, because they already take a body.

```lisp
(defn distance (a b)
  (local dx (- (:x a) (:x b)))
  (local dy (- (:y a) (:y b)))
  (sqrt (+ (* dx dx) (* dy dy))))

(defn total ()
  (let (x 5
        y 7)
    (+ y x)))
```

```rust
fn distance(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

fn total() -> i64 {
    let x: i64 = 5;
    let y: i64 = 7;
    y + x
}
```

`let-try` takes the same binding list and follows the same rules, and wraps each value in `try`. A `defrule` prototype of it on `main` at `54057b9` emits:

```lisp
(type add-texts (fn ((ref str) (ref str)) (result i64 (parse-error i64))))

(defn add-texts (left right)
  (let-try (a (parse left)
            b (parse right))
    (ok (+ a b))))
```

```rust
fn add_texts(left: &str, right: &str) -> Result<i64, std::num::ParseIntError> {
    let a: i64 = left.parse::<i64>()?;
    let b: i64 = right.parse::<i64>()?;
    Ok(a + b)
}
```

`loop` takes the same binding list and follows the same rules, so each value sees the names bound before it, as in Clojure's `loop`. `recur` still replaces every value at once. A name given twice in one `loop` list stays `L0203`, as it is today, because `recur` passes one value per name, and the first binding of the name would take an argument that nothing reads. Today `loop` takes `((name value) ...)` and expands to a call of a `fn`, so a value cannot see an earlier name: `(loop ((top (* limit 2)) (step top)) ...)` reports `L0201` for `top`. A `defrule` prototype of the new list on `main` at `54057b9`, which pairs the values in parallel as today's `loop` does, emits the Rust that `loop` emits today, which is also the Rust the sequential `loop` must emit:

```lisp
(defn power-of-two (exponent)
  (loop (step 0
         total 1)
    (if (< step exponent)
      (recur (+ step 1) (* total 2))
      total)))
```

```rust
fn power_of_two(exponent: i64) -> i64 {
    let mut step: i64 = 0;
    let mut total: i64 = 1;
    loop {
        if step < exponent {
            (step, total) = (step + 1, total * 2);
            continue;
        } else {
            break total;
        }
    }
}
```

The simplest expansion of sequential values binds them with `let` and then starts the loop. On `main` at `54057b9` that emits a second copy of each name, as `let top_2: i64 = top;` and `let mut step_2: i64 = step;`, which hand-written Rust never writes. A `fn` called where it is written binds each parameter as a new name, so no prelude `loop` can avoid the copy, and the sequential `loop` needs a compiler change. The build makes it in the emitter: when a `fn` called where it is written takes a local as the argument of a parameter with the same name, and nothing reads that local from the call on, the `fn` body included, the parameter reuses the local's binding. A borrow or a closure of the local that a form before the call made, and that is still live at the call, counts as a read, so `(loop (count 0 start (ref count)) ...)` keeps its copy, because reusing the binding would assign to `count` while `start` borrows it, which rustc rejects with `E0506`. The rule renames nothing, so a call such as `((fn (k) (+ k 1)) n)` keeps its `let k: i64 = n;`, and a call after which the body still reads the local keeps its copy: after `(local n 0)`, the call `((fn (n) (if (< n 3) (recur (+ n 1)) n)) n)` followed by `(println n)` keeps `let mut n_2: i64 = n;`, because reusing the binding would print `3` for the outer `n`. With that rule the prelude can express the sequential `loop`, so `loop` stays a prelude macro, and the build emits each value into the loop's own binding:

```lisp
(defn countdown-sum (limit)
  (loop (top (* limit 2)
         step top
         total 0)
    (if (> step 0)
      (recur top (- step 1) (+ total step))
      total)))
```

```rust
fn countdown_sum(limit: i64) -> i64 {
    let top: i64 = limit * 2;
    let mut step: i64 = top;
    let mut total: i64 = 0;
    loop {
        if step > 0 {
            (step, total) = (step - 1, total + step);
            continue;
        } else {
            break total;
        }
    }
}
```

`local` and `let` name two concepts: a binding that lasts to the end of the body, and a binding that takes its own body. Two concepts take two names, as `reverse` and `reverse!` name two operations, and one name never covers two concepts. Each other candidate reads as something it is not:

| Name | Why not |
| --- | --- |
| `def` | One name for two concepts with different rules, as option B shows. |
| `var` | Reads as a mutable binding, as it is in JavaScript and Janet. |
| `val` | Reads as the singular of `vals`, which reads a map's values. |
| `set` | Reads as the plain form of the prelude's `set!`, which assigns a place. |
| `bind` | Both forms bind, so the name does not tell them apart. |

`local` clashes with no prelude name and says what the binding is: a local of the enclosing body. Fennel, a Lisp that compiles to Lua, has the same pair: `let` encloses a body, and `local` binds to the end of the enclosing scope, as Lua's `local` does. The name rests on its meaning, so Fennel supports the choice without deciding it.

`local` stays a kernel form, because a macro sees only its own form and cannot bind a name for the forms that follow it in the enclosing body. The build records that reason in `docs/language.md`.

A `comment` never expands, so its old forms report nothing, and the build migrates them by hand. A `deftest` body is a function body, so its steps use `local`. `assert` prints the `let` as written. `infer` takes a scoped `let` as its operand and reports its value's type.

The old flat form gets its own error, with a third new macro-stage code, `L0336`, because `x` in `(let x 5)` is not a binding list. The message reads "`let` takes a list of bindings". When only a name and a value follow `let`, as old code always has, the help reads "to bind `x` for the rest of the body, write `(local x 5)`; to define it for the module, write `(def x 5)`". One help covers every position, because a macro sees only its own form and the `code` module does not say where the call sits, and an expression at the prompt is a body. When more forms follow, as in `(let x 5 (* x 2))`, the help shows the bindings in a list, `(let (x 5) (* x 2))`. `let-try` and `loop` report the same code for their own form, and their help shows the bindings in a list, as `(loop (index 0) ...)` for `(loop index 0 ...)`, because neither has a flat form. `(let)` and `(let x)` are `L0336` with the help `(let (name value) body ...)`, and an `L0336` whose first form is not a name, as `(let "a" 5)`, gets the same help, so no help prints `(local "a" 5)`. `let-try` and `loop` name their own form in that help. A list that lacks a value reports `L0334` before `L0335`, as `(let (x))` does. In a list with an even number of items, a binder whose second item is a literal, which no type is, as `(step 0)` in `((step 0) (total 1))`, is `L0336` too, with its own message, because the user did write a list: the message says that each name and its value go straight into the list, without their own parentheses, and the help prints the user's list in the new shape, as `(loop (step 0 total 1) ...)`. `L0227` becomes "`local` can only appear directly in a body", with the help "wrap the code that uses it in `(let (x 5) ...)`", which shows the binding as written. The typed old form, `(let (year i64) 2025)`, is a well-formed scoped `let` that binds `year` to the value `i64`, so the binding list check does not catch it. When a name in a value is unbound and names a type, as `i64` in `(let (year i64) 2025)`, `option` in `(option i64)` and `string` in `(vec string)` do, `L0201` keeps its code and its similar-name suggestion and adds the note "`i64` is a type; a type goes beside a name in a binding, as in `(local (year i64) 2025)`, or in a `type` signature". Only the leading name is the user's, and the example after it is fixed text, so a `string` binder is not shown `(local (year string) 2025)`. The note names where a type goes, so it fits every position, such as `(parse text i64)`. It appears once in each binding value and once in each other form of a body, on its first such name, so `(option i64)` reports `L0201` for `option` and for `i64` with one note between them. A name that names a macro keeps `L0304` unchanged, because `(map char numbers)` passes the macro `char` where a function goes, and a note about the type `char` would not help there. The expander adds the note wherever such a name appears, because only the compiler knows which names are types, so the rule does not depend on how the prelude `let` expands. A value that resolves stays valid, so `(let (make pair) ...)` still binds `make` to the function `pair`, which also names a type. At a file's top level, `(local x 5)` is `L0207`, as a flat `let` there is today, because a module body accepts only definitions. A scoped `let` around definitions, as `(let (rate 3) (defn scale (n) (* n rate)))`, expands to a module-level `do` that holds a `local`, so it reports `L0207` for the whole form and then `L0201` for `rate` inside `scale`. `rslisp repl` runs each expression input in a generated `main`, so `(local x 5)` binds for that input only and prints nothing, because RFC 0047 rejected a warning for a flat binding that is a whole input. A value that outlives one input is a `def`.

The migration renames each flat `(let ` to `(local `: 2,743 uses in 889 files under `prelude/`, `tests/` and `examples/`, 119 of them in the prelude. Flat bindings keep their meaning under the new name, so no code changes shape, except where a body binds a variable named `local`. Such a variable shadows the form for the rest of its body, as `(let if 3)` shadows `if` today, so a later `(local x ...)` there becomes a call. Eight fixtures bind a variable named `local`, and the build renames those variables. The rename also covers the compiler's own text: the helps of `L0208`, `L0404`, `L0407` and `L0410` suggest a flat `let` today, and the Rust unit tests in `src/ownership/` and `src/check.rs` and two `eval` tests in `tests/cli.rs` compile program text that writes one. A help that still names the flat `let` sends the user to an error, so the build reads each changed help line in the snapshots. The printer behind `rslisp expand` and the expand fixtures writes the kernel form's name as the literal `"let"` in `expand/src/core/printer.rs`, so the build reads the name from `KernelForm` there and the printed form stays one the reader accepts. `prelude/prelude.rslisp` states the kernel form's `(header-shape (let name))` and `(document (let name value) ...)`, and `KernelForm` holds its shape text, so all three move to `local`, and the prelude `let` states its own `(header-shape (let bindings) :bindings)`, and the shapes of `loop` and `let-try` add the marker. Only the prelude `let` expands to `local`, and `defmacro` builds `local` for the steps of a macro's body. The other prelude macros that emit a flat binding today, such as `let-try`, `the` and the comparison chains in `prelude/number.rslisp`, expand to `let`, as style rule 4 says. Five prelude sites build a flat binding as data, with `(quote let)`, where a text rename does not reach them: `pattern-lets` and `macro-definition` in `prelude/macros.rslisp`, `temporary-bindings` in `prelude/syntax.rslisp`, and `assoc-in` and `field-step` in `prelude/assoc.rslisp`. The `hygiene_capture` expand fixture builds one too, in `my-or`, and builds `local`, because its binding is a step of the `do` it builds. `defmacro` expands through the first two, and each binding they build is a step of the macro's body, so by style rule 1 those two build `local`. The other three build one `let` around the forms that follow the binding. The 21 uses of `let-try` in 11 files, one of them in `tests/cli.rs`, change shape, from `((a (parse left)) (b (parse right)))` to `(a (parse left) b (parse right))`, and 63 of the 77 uses of `loop`, in 26 files, change the same way. The other 14 write `(loop () ...)`, which reads the same in both shapes. The build then runs `rslisp fmt` over those paths, because `local` is 2 columns longer, which moves lists across the width limits, the `:bindings` layout moves every `let-try` and `loop` list, and the `source-format` check covers those paths.

### B: `def` for the flat form

The kernel form `let` becomes `def` inside a body, as Racket's internal `define` and Janet's `def` are, and `let` is scoped as in A.

```lisp
(defn distance (a b)
  (def dx (- (:x a) (:x b)))
  (def dy (- (:y a) (:y b)))
  (sqrt (+ (* dx dx) (* dy dy))))
```

The Rust is the same as in A. One word would then follow two sets of rules:

| | top-level `def` | `def` in a body |
| --- | --- | --- |
| Order | order-independent, like Rust items | sequential |
| Types | a function generalizes to Rust generics | monomorphic, because Rust closures cannot be generic |
| Moves | holds one value, and a use borrows it (`L0503`) | can be moved |
| Rebinding | one definition per name | can shadow |

The Lisps that use one form at both levels keep one set of rules at both levels:

| Lisp | Form | Top level | Body |
| --- | --- | --- | --- |
| Scheme, Racket | `define` | definitions may refer to each other in any order | definitions may refer to each other in any order, as `letrec*` does |
| Janet | `def`, `var` | sequential | sequential |
| Fennel | `local` | sequential, local to the file | sequential |
| Hy | `setv` | Python assignment | Python assignment |

Common Lisp and Clojure keep body bindings in `let`, and a `defun` or a `def` inside a function defines a global. The top level of this language follows Racket's order rule and its bodies follow Janet's, so `def` would be the only shared form whose rules change with its position. A nested Rust `fn` item would give a body-level function the top-level order rule and generics, but it cannot capture a local, so a closure over a local would still follow the body rules. At a REPL prompt, `(def x 5)` would outlive the input while a `def` inside an expression would not.

### C: a scoped `let` only

`let` is scoped as in A, and the language has no flat form. A function that binds in steps nests one level for each group of bindings that code separates:

```lisp
(defn report (n)
  (let (half (/ n 2))
    (println half)
    (let (quarter (/ half 2))
      (println quarter))))
```

The Rust is the same as in A. `let-try` and every other prelude macro that splices a binding into a body would nest in the same way.

### D: keep the flat `let`, and scope with another name

`let` keeps its flat meaning, and a new prelude macro such as `with` takes the binding list and a body.

```lisp
(with (x 5
       y 7)
  (+ y x))
```

The Rust is the same as in A. No program changes.

## Recommendation

Option A.

- A Lisper reads `let` as Common Lisp, Scheme, Racket and Clojure do, and the `let-` prefix means one thing across `let`, `let-pair`, `let-fields` and `let-try`.
- The flat form keeps step-by-step function bodies as short as they are today, and a Rust reader recognizes its shape.
- Each name follows one set of rules. A `def` is a top-level definition everywhere, and a `local` is a body binding everywhere.
- It keeps "small compiler, big prelude": the scoped `let`, `let-try` and `loop` lists are prelude macros, as the prototypes show. Beyond renaming one kernel form, the compiler changes in five places: the Rust that a sequential `loop` emits, a note on `L0201` for a name that is a type, the `L0227` help, which prints the binding as written, the `:bindings` layout in `rslisp fmt`, and the three new macro-stage codes, `L0334` for a name without a value, `L0335` for a form with no body and `L0336` for a binding list that does not alternate names and values, as in `(let x 5)` and `((step 0) (total 1))`, which a prelude `fail` can name only once `expand/src/diagnostic/code.rs` lists them. The marker also changes `shape-marker-names`, `marker-help` and the help of `extra-header-shape-marker` in `prelude/macros.rslisp`, and the sentence in `docs/language.md` that `:docstring` and `:one-per-line` are the only keywords the formatter names.
- The migration of flat bindings renames forms without changing their structure. The uses that change shape are the `let-try` and `loop` lists, the prelude macros of style rule 4, the other prelude `(do (let ...) ...)` forms of style rule 2, in `map-form` and `display-body`, the three `(quote let)` builders that build a scoped `let`, in `assoc-in`, `field-step` and `temporary-bindings`, and the variables named `local`.

A has costs:

- The language has two binding forms, so the prelude, the docs and the examples need a style rule for choosing between them. The Decision section records it.
- An old `loop` or `let-try` list with an even number of bindings reads as typed binders and values, and so does a Scheme style `(let ((x (f)) (y (g))) ...)`. Only a binder whose second item is a literal shows the mistake, so when no binder has one, no error names the new list: `((a (parse left)) (b (parse right)))` reports that the type `parse` cannot be found and that `b` cannot be found, and then that `a` and `b` cannot be found at each use in the body, although the list writes both names.
- The old flat form error binds nothing, so each later use of its name reports `L0201`, and the similar-name help can offer a kernel form, as `do` for `dx`. `defn`, `defmacro`, `defrule` and `defstruct` declare their names before their own checks, so a failed definer adds no such report, but `let` is not a definer, and every other prelude macro that fails leaves its names unbound in the same way, so the fix, one rule that does not report a name that a failed macro call earlier in the body wrote, belongs to a later change for every macro.
- A top-level `let` that shares a value between definitions, as a Common Lisp or Clojure reader writes it, reports `L0207` and then `L0201` for each use of its names, and neither error names the scope rule.
- A typed old form whose type is applied, as `(let (parsed (result i64 string)) ...)`, reports `L0201` once for each name in the type, with one note.
- A typed old form whose type is also a prelude macro, as `(let (letter char) ...)`, reports `L0304` as today, which does not name `local`. `str` answers for its name in value position, so `(let (name str) ...)` binds `name` to a function, and it compiles when the body calls `name` and reports `L0407` otherwise.
- A typed old form whose type is a function type, as `(let (pick (fn (i64) i64)) ...)`, reads as a binding of a valid closure, `|i64: i64| i64`, so no error names `local`, and the next use of `pick` reports `L0201`.
- A program is likelier to name a variable `local` than `let`. Such a variable shadows the form for the rest of its body, and a later `(local x ...)` then reports `L0201` for `x` without naming the shadowing.

## Decision

The project owner chose the binding list on 2026-11-02:

```lisp
(let (x 5
      y 7)
  (+ y x))
```

The owner chose option A on 2026-11-02: the flat form is `local`, and `let` is a scoped prelude macro.

The owner decided on 2026-11-02 that `let-try` keeps its name and takes the same binding list as `let`:

```lisp
(let-try (right (pop-value! stack)
          left (pop-value! stack))
  (push! stack (+ left right)))
```

The owner decided on 2026-11-02 that `loop` takes the same binding list as `let` and that its values see the names bound before them, so every binding list in the language has one shape and one meaning:

```lisp
(loop (top (* limit 2)
       step top
       total 0)
  ...)
```

The owner decided on 2026-11-02 that a name given twice in one `loop` list stays `L0203`, as it is today, because `recur` passes one value per name. This is the one rule in which a `loop` list differs from a `let` list.

The owner decided the style rule on 2026-11-02: bind the steps of a function body with `local`, and use `let` where only one expression fits or where a binding should end before the body does. The prelude, the docs and the examples follow it:

1. **The steps of a function body use `local`.** They add no nesting and read as the Rust they emit.
2. **A position that takes one expression uses `let`:** an `if` branch, a `match` arm or an argument. The prelude writes 9 `(do (let ...) ...)` forms that become a `let`.
3. **A binding that only the next few forms use, or whose name would mislead later in the body, uses `let`.**
4. **A macro template that binds names for its own forms uses `let`,** by rule 2, because a template is one expression. `let-try`, `the` and the comparison chains in `prelude/number.rslisp` write `(let ...)` where they write `(do (let ...) ...)` today. Only the prelude `let` expands to `local`, and `defmacro` builds `local` for the steps of a macro's body, by rule 1. The bodies of the `defrule` machinery that `let` is built with keep `(do (local ...) ...)`, as `fill-repeated` in `prelude/macros.rslisp` does, because a `let` there would run while its own expansion is being built and report `L0307`.

Racket's style guide prefers internal definitions over `let` in a body for the same reason as rule 1: each `let` indents its body one more level. Rust writes flat `let` statements and opens a block only to end a scope early or to compute a value where an expression goes.

The owner accepted the rules listed under Options on 2026-11-02. A form that is always a mistake is an error: a `let` with no body, a name without a value, a keyword as a binder, and a binding list that is not a list, as in the old `(let x 5)`. A form with a real use stays allowed: an empty binding list, which a macro builds from zero bindings, and a later binding that shadows an earlier one in the same list, which refines a value step by step as Rust's `let x = x.trim();` does. Bindings that nothing uses, such as `(let (x 5) 7)`, are left to a later RFC, because one check for them covers `let`, `local`, `let-try` and function parameters alike.

The owner decided the binding-list layout on 2026-11-06: a binding list with two or more pairs always breaks, one name and its value per line, as in the RFC's own example. A single pair stays on the `let` line. A pair never splits across lines, however long. The rule covers `let`, `let-try` and `loop`, and any user macro that opts in, through a header-shape marker. The marker is `:bindings`.

```lisp
(let (x 5) (+ x 1))

(let (x 5
      y 7)
  (+ y x))
```

RFC 0053's rule 2, its rule 5 and its run filling no longer apply to a binding list.

These rules complete the layout:

- A comment on its own line takes the indentation of the next name and never counts as a pair.
- A trailing comment after a name moves to the end of that pair's last line.
- A comment right after `(` keeps pairs together.
- A value that breaks starts on its name's line, and its later lines lay out by its own shape, relative to the value's first column.
- `:bindings` applies to the header's last form, which the shape names, appears at most once, and may join the other markers. Any other form after the shape is `L0331`, and a misspelled marker's help suggests the closest marker.
- `(let x 5)` lays out as a call, because the form after the head is not a list. A list with an odd number of items lays out by pairs, with its last name alone.

## Rejected options and why

The project owner chose A on 2026-11-02.

- **B: `def` for the flat form:** one name for two concepts with two sets of rules, where each concept should have its own name. Every Lisp that shares one form between the top level and a body keeps one set of rules for it, and a different name tells the reader that different rules apply. A Clojure user reads a `def` in a body as a global, which Clojure's `def` creates. At a REPL prompt, the same word would outlive an input in one position and not in another.
- **C: a scoped `let` only:** every function that binds in steps nests one level per group, while the emitted Rust stays flat. The language loses a form that reads like the Rust it emits.
- **D: keep the flat `let`, and scope with another name:** it keeps the surprise for every Lisper, and `let` stays the one `let-` form without a body.

## Rule

[docs/language.md: Kernel forms](../docs/language.md#kernel-forms), [Bodies](../docs/language.md#bodies), [Macros](../docs/language.md#macros), [Header shapes](../docs/language.md#header-shapes), [Hygiene](../docs/language.md#hygiene), [Evaluation](../docs/language.md#evaluation), [Source formatting](../docs/language.md#source-formatting), [8.4 Loops](../docs/language.md#84-loops) and [9.1 Propagating errors with `try`](../docs/language.md#91-propagating-errors-with-try). The build rewrites those sections, the `let` row of the kernel forms table, and every other rule and example of the flat `let` in `docs/language.md`, including the scope rules under Hygiene. The style rule of the Decision section lands in the Bodies section, its rule 4 and the exception for the `defrule` machinery land in the Macros section, and the `:bindings` marker and its layout land in the Header shapes and Source formatting sections.
