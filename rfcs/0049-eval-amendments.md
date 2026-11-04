# RFC 0049: Amendments to `rslisp eval`

**Status:** Accepted
**Date:** 2026-11-04

## Problem

RFC 0047 gives `eval --json` a `note` object for a line that `eval` prints itself: the type of a value that does not print, or that the module defines a form. The build of `eval` made each of those lines a `Diagnostic` with a span and no code, so the compiler already reports them as it reports a warning. A separate `note` object would give an editor two shapes for one kind of message, and would drop the span that places the note in the buffer.

RFC 0047's rule 4 also has `eval` run nothing only when a module diagnostic lies inside the forms. The build runs nothing whenever the module has an error, as `docs/language.md` states, and RFC 0047 does not record that choice.

RFC 0047's rule 5 prints a form's value with one inline statement, `(println "<marker>" (pr-str form))`. The forms that print are known only after the type check, and the expansion that lowered the forms is closed by then, so the inline statement cannot be lowered in the body of the entry without more work.

RFC 0047's rule 5 also has `eval` hold back as many bytes as the marker is long, so a marker split across two reads is still found. A program that prints a prompt with no newline and then waits for input would then show the prompt without its last bytes until the program prints more.

## Options

### Notes as diagnostics

A note is a `Diagnostic` with severity `note`, a span and no code. In `--json` it goes in `diagnostics`, as LSP's information severity does:

```json
{"diagnostics": [{"severity": "note", "message": "the module defines this form, so it does not run", "file": "geo.rslisp", "line": 1, "column": 1, "end_line": 1, "end_column": 41, "rendered": "note: the module defines this form, so it does not run ..."}]}
```

### A separate `note` object

RFC 0047's table: `{"note": "the module defines this form, so it does not run"}`, a line of text without a place.

### A module error runs nothing

When the module has any error, `eval` reports the module's diagnostics and runs no form.

### Run the forms that reach no broken definition

`eval` runs a form whose definitions all check, and reports the module's errors beside its results.

### One generated generic function that takes the value

The expansion lowers one function, `(fn (value) (println "<marker>" (pr-str value)))`, in the prelude's scope, and each printed form becomes `(print-value form)`. The checker generalizes the function over the value, so it prints forms of every type in one call:

```lisp
(square 3) (->point :x 1 :y 2)
```

```rust
fn print_value<A: std::fmt::Debug>(value: &A) {
    println!("\u{1e}rslisp<hex>{:?}", value)
}

fn main() {
    print_value(&square(3));
    print_value(&Point { x: 1, y: 2 });
}
```

### A second module expansion per call

`eval` expands the module again after the check, with the printed forms wrapped inline. It makes `eval` 2% to 31% slower on the measured modules.

### Keep the expansion open across the check

The check runs on a copy of the program while the expansion stays open. The module check, the type query and the notes must then move inside the expansion, a change of more than 200 lines.

### An IR walker for the inline shape

The expansion lowers `(println "<marker>" (pr-str hole))` once and puts each form's expression in place of `hole`. It needs a walker over every kind of expression, with a test for each, 150 to 200 lines.

### A `String` per printed value

The generated function takes the `pr-str` text, `print_value(&format!("{:?}", x))`. Each printed value allocates a `String` that neither the source nor rule 5 writes.

### Hold back a marker's length

`eval` holds back the last bytes of the output, as many as the marker is long, until the next read.

### Hold back only a start of the marker

`eval` holds back only the longest end of the output that is also a start of the marker. The marker starts with the record separator, `U+001E`, which program text rarely ends with, so a prompt such as `Your answer` streams whole.

## Recommendation

Notes as diagnostics, because one shape covers every message the compiler reports and every message keeps its place. A module error runs nothing, because the emitter needs a whole checked program and the finer rule needs reachability from each form, which is a step of its own. Hold back only a start of the marker, because it still finds a split marker and lets a prompt show at once. Print through one generated generic function, because it keeps rule 5's single `println`, adds no allocation, and needs neither a second expansion nor an IR walker.

## Decision

The project owner decided each on 2026-11-04:

1. **Notes are diagnostics.** A note, such as "the module defines this form, so it does not run" or a value whose type does not print, is a `Diagnostic` with a span and no code. In `--json` it goes in `diagnostics` with `"severity": "note"` and no `code`, as LSP's information severity does, not in RFC 0047's separate `note` field.
2. **A module error runs nothing.** When the module has any error, `eval` reports the module's diagnostics and runs no form, as `docs/language.md` states today. A finer rule that runs forms which reach no broken definition is a later step.
3. **`eval` holds back only an end of the output that could start the marker.** Output that cannot start the marker streams at once.
4. **Printed forms go through one generated generic function that takes the value.** Each printed form becomes `(print-value form)`, and `print-value` is `(fn (value) (println "<marker>" (pr-str value)))`. It keeps rule 5's single `println` and adds no allocation.

These decisions supersede RFC 0047's rule 4 on a module error, the `note` row of its rule 5, rule 5's hold-back of a marker's length, and rule 5's inline `(println "<marker>" (pr-str form))`.

## Rejected options and why

- **A separate `note` object:** it gives an editor a second shape for one kind of message and loses the span.
- **Run the forms that reach no broken definition:** deferred, not rejected. It needs a reachability check from each form before emission.
- **Hold back a marker's length:** a prompt without a newline would lose its last bytes until the program prints more.
- **A second module expansion per call:** it makes `eval` 2% to 31% slower.
- **Keep the expansion open across the check:** it moves the module check, the type query and the notes inside the expansion, more than 200 lines.
- **An IR walker for the inline shape:** 150 to 200 lines for the same Rust that the generic function gives.
- **A `String` per printed value:** an allocation that the source does not write.

## Rule

[docs/language.md: Evaluation](../docs/language.md#evaluation)
