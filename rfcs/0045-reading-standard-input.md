# RFC 0045: Reading standard input

**Status:** Accepted
**Date:** 2026-11-02

## Problem

The prelude reads standard input in two ways. `slurp-stdin` returns all of it at once, and `read-line` returns one line per call. `slurp-stdin` is a second name for what `slurp` does: the only difference is the source that it reads.

No form streams standard input line by line. A filter such as `grep` or `wc` must either hold all of its input in memory:

```lisp
(defn main ()
  (let-try ((text (slurp-stdin)))
    (do (println (count (lines (as-str text)))) (ok (do)))))
```

or write a recursive `read-line` loop:

```lisp
(defn count-lines (total)
  (match (read-line)
    ((ok (some _)) (count-lines (+ total 1)))
    ((ok none) (ok total))
    ((err error) (err error))))
```

## Options

### A: `stdin` as a value, `slurp` over a source trait, and `line-seq`

`stdin` names both a type and a value. `slurp` takes any type of the runtime's `source` trait, which `str`, `string` and `stdin` implement. `slurp` takes the source by reference, so an owned path stays usable after the read. `line-seq` streams standard input and yields each line as a `result`. After a failed read it yields that `err` and ends, because Rust's `Lines` returns the same error forever when a read keeps failing, and `for-each` cannot stop it.

```lisp
(rust-type stdin "std::io::Stdin")
(def stdin (rust "std::io::stdin()" stdin))

(rust-trait source "crate::rt::Source" str string stdin)
(def slurp (rust "crate::rt::Source::slurp({0})"
                 (where ((s source)) (fn ((ref s)) (result string io-error)))))

(def line-seq (rust "crate::rt::lines({0})"
                    (fn ((ref stdin)) (iterator (result string io-error)))))
```

```lisp
(defn main ()
  (for-each (line-seq stdin)
    (fn (line)
      (match line
        ((ok text) (println (len text) " " text))
        ((err error) (eprintln error))))))
```

```rust
crate::rt::lines(&std::io::stdin()).for_each(
    |line: Result<String, std::io::Error>| match line {
        Ok(text) => println!("{} {}", text.len(), text),
        Err(error) => eprintln!("{}", error),
    },
)
```

`(slurp stdin)`, `(slurp "cars.txt")` and `(slurp path)` on an owned `string` emit:

```rust
crate::rt::Source::slurp(&std::io::stdin())
crate::rt::Source::slurp("cars.txt")
crate::rt::Source::slurp(&path)
```

The runtime's `Source` part, `prelude/runtime/source.rs`, implements the trait for `str`, `String` and `std::io::Stdin`. The runtime's `Lines` part, `prelude/runtime/lines.rs`, reads each line with the `read_line` of the `Io` part, so it locks standard input for one line at a time and `read-line` and `(slurp stdin)` may read between its lines. It ends the stream after its first error.

A prototype that took the source by value failed: a `string` path did not satisfy a trait implemented for `(ref string)`.

### B: one `lines` over strings and standard input

`lines` takes a string or `stdin` through a trait with an associated line type: a `(ref str)` for a string and a `(result string io-error)` for standard input.

```lisp
(for-each (lines stdin) print-line)
(count (lines (as-str text)))
```

The prototype ran on both sources. A function that calls `lines` on an untyped parameter failed:

```lisp
(defn nonblank (text) (filter (lines text) (fn (line) (not (empty? line)))))
```

```text
`(line a)` does not implement `sequence`
```

The line type stays a projection until a caller fixes the source, so `empty?` cannot check it inside `nonblank`. The fixture `run/puzzles/sonar.rslisp` failed the same way.

### C: keep `slurp-stdin` and add a streaming form

`slurp-stdin` stays beside `slurp`, and a new form streams standard input.

```lisp
(slurp-stdin)
```

```rust
std::io::read_to_string(std::io::stdin())
```

### D: name standard input `*in*`

Standard input is the value `*in*`, as in Clojure.

```lisp
(slurp *in*)
```

```rust
crate::rt::Source::slurp(&std::io::stdin())
```

### E: stop the stream at the first error

`line-seq` yields a `string` per line and ends at the first failed read.

```lisp
(for-each (line-seq stdin) println)
```

```rust
std::io::BufRead::lines(std::io::stdin().lock()).map_while(Result::ok)
```

## Recommendation

Option A. One name, `slurp`, reads every source, and `line-seq` streams standard input with one line in memory. Each read failure reaches the program as an `err`, and a read that keeps failing ends the stream instead of looping. `lines` keeps working on any string, an untyped parameter included.

The cost: `slurp` now emits `crate::rt::Source::slurp(path)` instead of `std::fs::read_to_string(path)`, and a program that calls `slurp` carries the runtime's `Source` part: the trait and its three implementations. The trait has a runtime part of its own because a program that called `slurp` and not `read-line` would otherwise carry an unused `read_line`, and clippy rejects that dead code in the emitted Rust.

## Decision

The project owner accepted option A. Review found that the plain `std::io::BufRead::lines` loops forever on a read that keeps failing, such as a directory as standard input, so the owner chose that `line-seq` yields the first `err` and then ends. A line that is not valid UTF-8 is such an `err` too, so `line-seq` reads no line after it, although plain `Lines` would go on to them, and a later `read-line` reads the next one. `slurp-stdin` is removed, and a program that called it now writes `(slurp stdin)`. `lines` stays a function of strings only.

## Rejected options and why

- **B: one `lines` over strings and standard input:** the line type stays a projection until a caller fixes the source, so a function that calls `lines` on an untyped parameter fails, as `nonblank` and `run/puzzles/sonar.rslisp` did.
- **C: keep `slurp-stdin`:** it is a second name for `slurp`.
- **D: name standard input `*in*`:** in Clojure the earmuffs mean a dynamic variable that `binding` rebinds, and rslisp has none.
- **E: stop the stream at the first error:** it hides a failed read.

## Rule

[docs/language.md: Standard types](../docs/language.md#standard-types)
