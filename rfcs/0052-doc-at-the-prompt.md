# RFC 0052: `doc` at the prompt

**Status:** Accepted
**Date:** 2026-11-04

## Problem

`(doc term)` at the prompt prints the page as one quoted string, with `\n` escapes:

```text
> (doc defn)
"(defmacro defn (name parameters body ...))\n\nDefines a function: (defn name (parameter ...) body ...).\n\nA string before at least one more body form is the docstring."
```

`rslisp doc defn` prints the same page as text:

```text
(defmacro defn (name parameters body ...))

Defines a function: (defn name (parameter ...) body ...).

A string before at least one more body form is the docstring.
```

The project owner reported on 2026-11-04 that the doc should print cleanly at the prompt. RFC 0047 chose the quoted string: `(doc term)` is an ordinary form that expands to the page as a string literal, and the prompt prints every result through `prn`. RFC 0048 then fixed what `prn` prints: a string prints as the string literal that the reader reads, on one line, so a newline prints as `\n`. Both rules are right for values, and together they make a page unreadable.

The language cannot fix this today without one of three changes: `doc` stops returning a string, the page gets a type of its own, or the prompt prints some results differently. A fourth change, the prompt treating `doc` by name in Rust, is ruled out by `AGENTS.md`: it makes the prompt evaluate one input differently by its head, so the prompt depends on how the prelude builds the page.

The probes below ran on a release build of `main` at `5e285949`, built with `nix develop -c cargo build --release`, on Linux. The prototype of option A is a user macro typed at the prompt, `(defmacro print-doc (term) (template (println (doc (unquote term)))))`, which expands to what option A's `doc` expands to.

| Probe | Output |
| --- | --- |
| `(doc square)` at the prompt | `"(defn square (n))\n\nMultiplies a number by itself."` |
| `(println (doc square))` at the prompt | the page as text, then nothing: a `unit` result prints nothing |
| `(print-doc square)` at the prompt | the page as text, then nothing |
| `(str (print-doc square))` at the prompt | `L0404`: `unit` does not implement `display` |
| `rslisp eval --json m.rslisp '(doc square)'` | `{"value":"\"(defn square (n))\\n\\nMultiplies a number by itself.\"","form":1}` |
| `rslisp eval --json m.rslisp '(print-doc square)'` | `{"out":"(defn square (n))\n\nMultiplies a number by itself.\n"}` |
| a `page` struct with a `display` impl, at the prompt | `(->page :text "(defn square (n))\n\nMultiplies a number by itself.")` |
| `(str "a" "b")` at the prompt | `"ab"` |

## Prior art

Every lookup below that the user types at a prompt prints the page and shows no result value. Each language keeps a separate way to get the text as data.

| Lookup | At the prompt | The text as data |
| --- | --- | --- |
| Clojure `(doc f)` | a macro that calls `print-doc`: a line of dashes, the name, the arglists, then the docstring, through `println`, with the arglists through `prn`. Returns `nil`, which the REPL prints. A `def` runs at once, so `(def p (doc f))` prints the page at the `def` and binds `nil`. | `(:doc (meta #'f))` |
| Clojure `(source f)` | `(println (or (source-fn 'f) "Source not found"))`, returns `nil` | `(source-fn 'f)` |
| Python `help(f)` | renders the page and sends it to a pager, returns `None`, which the prompt does not print | `pydoc.render_doc(f)`, `inspect.getdoc(f)`, `f.__doc__` |
| IPython `f?`, `f??` | the `%pinfo` magic: `Inspector.pinfo` sends the page to `page.page` and returns nothing, so no `Out[]` line shows | `f.__doc__` |
| Julia `?f` | a help mode of the prompt that renders the docs | `@doc f` returns an object that "will by default display itself intelligently": the prompt shows it as rendered text, not as a quoted string |
| IEx `h f` | prints the docs and returns `IEx.dont_display_result()`, the atom `:"do not show this result in output"`, which the prompt skips | `Code.fetch_docs(module)` |

Clojure, Python, IPython and IEx print from the lookup itself and return a value the prompt hides or prints as `nil`. Only Julia returns a value of a documentation type whose display is the rendered page, and Julia's display has a channel per media type, `text/plain` among them, which is separate from the printed form of a value.

Most prompts print a string result in quotes: Python and Node.js (`'ab'`), Clojure, IEx, GHCi and Julia (`"ab"`). Some print it raw: PowerShell (`"a" + "b"` prints `ab`), `tclsh` (`set x "a b"` prints `a b`) and Lua 5.4, whose prompt passes each result to the global `print`. R prints `[1] "ab"` and leaves raw text to `cat`.

Sources:

- **Clojure** 1.12.0: [`clojure/repl.clj`](https://github.com/clojure/clojure/blob/clojure-1.12.0/src/clj/clojure/repl.clj) (`print-doc`, `doc`, `source-fn`, `source`), [`clojure/main.clj`](https://github.com/clojure/clojure/blob/clojure-1.12.0/src/clj/clojure/main.clj) (the REPL prints through `prn`).
- **Python** 3.13: [`pydoc.py`](https://github.com/python/cpython/blob/v3.13.0/Lib/pydoc.py) (`Helper`, `render_doc`), [`inspect.getdoc`](https://docs.python.org/3/library/inspect.html#inspect.getdoc), [`sys.displayhook`](https://docs.python.org/3/library/sys.html#sys.displayhook).
- **IPython**: [`IPython/core/oinspect.py`](https://github.com/ipython/ipython/blob/main/IPython/core/oinspect.py) (`Inspector.pinfo`), [`IPython/core/magics/namespace.py`](https://github.com/ipython/ipython/blob/main/IPython/core/magics/namespace.py).
- **Julia**: [Documentation](https://docs.julialang.org/en/v1/manual/documentation/), [the REPL's help mode](https://docs.julialang.org/en/v1/stdlib/REPL/).
- **IEx** 1.17.3: [`iex/introspection.ex`](https://github.com/elixir-lang/elixir/blob/v1.17.3/lib/iex/lib/iex/introspection.ex) (`h`), [`iex.ex`](https://github.com/elixir-lang/elixir/blob/v1.17.3/lib/iex/lib/iex.ex) (`dont_display_result`), [`Code.fetch_docs/1`](https://hexdocs.pm/elixir/Code.html#fetch_docs/1).
- **Raw strings**: [PowerShell `Out-Default`](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/out-default), [`tclsh`](https://www.tcl-lang.org/man/tcl8.6/UserCmd/tclsh.htm), [Lua 5.4.6 `lua.c`](https://github.com/lua/lua/blob/v5.4.6/lua.c) (`l_print`), [R `print.default`](https://stat.ethz.ch/R-manual/R-devel/library/base/html/print.default.html). The PowerShell and `tclsh` outputs come from their documentation, not from a probe.

## Options

Each option shows the prompt for `(defn square (n) "Multiplies a number by itself." (* n n))`.

### A: `doc` prints the page and returns `unit`

`doc` expands to a call of `println` on the page, as Clojure's `doc` prints and returns `nil`. A new prelude macro, `(doc-page term)`, takes today's place of `doc` and returns the page as a string literal. `header`, `docstring` and RFC 0050's `page-body` stay as they are.

```lisp
(defmacro doc (term)
  "Prints the page of a term: its header, a blank line, then its docstring."
  (template (println (doc-page (unquote term)))))

(defmacro doc-page (term)
  "Returns the page of a term as a string: its header, a blank line, then its docstring.

  A term without a docstring has `No documentation.` in its place."
  (documentation-part (quote page) term))
```

```text
> (doc square)
(defn square (n))

Multiplies a number by itself.
> 
```

`(defn main () (doc defn))` emits:

```rust
fn main() {
    crate::rt::restore_default_sigpipe();
    println!(
        "{}",
        "(defmacro defn (name parameters body ...))\n\nDefines a function: (defn name (parameter ...) body ...).\n\nA string before at least one more body form is the docstring."
    )
}
```

The page is a string literal in the binary, as it is today. The call formats it once through `Display` for `&str`, with no allocation and no `Show` code. Today the prompt prints the same literal through `prn`, which emits `crate::rt::Show` and its impls into the program and escapes each newline, quote and backslash. The expansion does the same lookup as today plus one `println` call, so compile time does not change in a way the probes could measure.

- **At the prompt:** the page prints as `rslisp doc` prints it. The result is `unit`, which the prompt prints as nothing, so no rule of the prompt changes. Under `eval --json` the page arrives as program output, `out`, not as a `value`, so an editor shows it where it shows the program's output.
- **In a program:** `(doc f)` prints the page at run time and returns `unit`. `(str (doc f))` is `L0404`, as the probe shows. A program or a test that needs the text writes `(doc-page f)`, `(header f)` or `(docstring f)`, which stay string literals, so `(value-eq (doc-page f) "...")` and `(str (doc-page f))` work.
- **In a `def`:** `(def p (doc f))` binds `unit`. The page prints when the first form of a run reads `p`, not at the `def`, because a top-level `def` lowers to a lazy `static`. At the prompt it prints again on each input that reads `p`. Write `(def p (doc-page f))` to keep the text.
- **Other terms:** an unbound term is `L0201` for both lookups, as today. A term with no docstring prints, or returns, its header, a blank line and `No documentation.`. `(doc-page f)` at the prompt prints the quoted string, as `(doc f)` does today, and under `eval --json` it comes back as a `value`.
- **`rslisp doc`:** the CLI expands `(doc-page term)` instead of `(doc term)` until RFC 0050's `(header term)` and `(page-body term)` land, then expands those two. It never runs a program. The CLI names the lookup macros it expands, `doc-page`, and later `header` and `page-body`, as 0041 D5 lets it name `doc`. The subcommand keeps the name `doc`.
- **Cost:** the 49 expand fixtures and `tests/cli.rs`'s `PAGE_PROGRAM` that use `(doc ...)` for its string switch to `(doc-page ...)`. `doc_unbound_term` keeps `doc`, and `docs/language.md` states the two lookups. The owner is the only user, so no migration aid is needed.

### B: `doc` returns a `page` value whose printed form is its text

The prelude declares a `page` type that holds the text. `display` writes the text, so `(str (doc f))` and `(println (doc f))` give the text. For the prompt to print it cleanly, the `show` impl of `page` must also write the raw text.

```lisp
(defstruct page :text (ref str))
(display page (value) (field value text))
```

```text
> (doc square)
(defn square (n))

Multiplies a number by itself.
```

Emitted Rust: the same `println!` of a field, plus a `Display` impl.

- **In a program:** `(doc f)` is a `page`. `(str (doc f))` gives the text. A test compares `(str (doc f))`, or the prelude gives `page` an `eq` impl.
- **The cost:** RFC 0048's rule is that a printed value is a form that the reader reads, on one line, so that `eval --json` finds the value as the rest of one line and RFC 0050 can color it by reading it back. A `show` that writes raw text breaks all three for one type. With a `show` that keeps the rule, the prompt prints `(->page :text "(defn square (n))\n\nMultiplies a number by itself.")`, as the probe shows, which is worse than today.
- With a `string` field, as in the probe, each `(doc f)` allocates at run time, which the source does not write.
- B is Julia's design, but Julia shows a value through a channel per media type, separate from its printed form. The language has one printed form, `show`, so B needs that second channel or an exception in `show`.

### C: the prompt prints a string result without quotes

The prompt prints a result of type `string` or `(ref str)` through `display` instead of `prn`. `doc` is unchanged. PowerShell, `tclsh` and Lua 5.4 print string results this way.

```text
> (doc square)
(defn square (n))

Multiplies a number by itself.
> (str "a" "b")
ab
> (str "")

> "42"
42
```

Emitted Rust: unchanged from today.

- **In a program:** nothing changes. `(doc f)` is a string, and `(str (doc f))` works.
- **The cost:** a string result no longer looks like a string. `"42"` and `42`, or `"none"` and `none`, print alike, and `""` prints an empty line. RFC 0048 chose printed forms that read back so that a user can paste a result into a program: a string result could no longer be pasted. The prompt and `eval --json` would print one value two ways. The rule is an exception keyed on two types, in Rust, against "PREFER a uniform rule that gives the better output OVER a special case".

### D: keep `doc` and add a printing lookup under another name

`doc` keeps returning the string. A new macro, such as `(help term)`, prints the page, as option A's `doc` does.

```text
> (help square)
(defn square (n))

Multiplies a number by itself.
> (doc square)
"(defn square (n))\n\nMultiplies a number by itself."
```

Emitted Rust: as A for `help`.

- **In a program:** as today.
- **The cost:** the form a user types first, `(doc square)`, which Clojure's habit and `rslisp doc` both suggest, keeps the unreadable output. The language gains a second name for the same page.

### E: the prompt treats `doc` by name

The prompt recognizes a `(doc term)` input and prints the page itself. This is ruled out: E makes the prompt evaluate one input differently by its head, so the prompt depends on how the prelude builds the page, which `AGENTS.md` forbids.

Emitted Rust: none, the prompt prints in Rust.

## Colors

RFC 0050 colors a page's header with the token colors and leaves the docstring plain. The CLI MUST NOT split the page string, so it expands `(header term)` and `(page-body term)` and colors only the header. That rule covers `rslisp doc`, and holds under every option: under A, `rslisp doc` stops expanding `doc` at all.

At the prompt, RFC 0050 colors two things: a result value, by reading its printed text back with the reader, and nothing in the program's output, which passes through unchanged.

- **Under A,** the page is program output, so it prints plain at the prompt, header included. No option colors the header there without the program knowing the color decision and the token rule, or without the prompt splitting the page.
- **Under B,** a raw `show` text is not a form, so the value colorer cannot read it back. The page prints plain, or the colorer needs an exception for `page`.
- **Under C,** a string result printed without quotes is not a form either, and coloring it as a string would turn the whole page green.
- **Under D,** as A for `help`, and as today for `doc`: one green string.

So under every option the page prints plain at the prompt, and only `rslisp doc` colors the header. That meets RFC 0050's decision 7, which colors the header of the page `rslisp doc` prints and leaves program output plain.

## Recommendation

Option A.

- It gives the output the owner asked for at the prompt, with no change to the prompt's rules: a `unit` result already prints nothing.
- It keeps RFC 0048's rule for every value. No type prints two ways.
- The compiler gains nothing. The CLI names the lookup macro it expands, `doc-page`, where it named `doc`, and the subcommand keeps the name `doc`. The prelude changes `doc` and adds `doc-page`.
- It follows Clojure, the language whose `doc` the user types by habit. A program that needs the text has `doc-page`, `header` and `docstring`, which return string literals.
- `rslisp doc` stops depending on `doc` returning a string, which RFC 0050's `header` and `page-body` complete.

A has costs:

- `(doc f)` in a program prints, and `(str (doc f))` is `L0404`.
- The page at the prompt prints plain, header included.
- `(def p (doc f))` binds `unit`, and the page prints when a form first reads `p`, not at the `def`. At the prompt it prints again on each input that reads `p`.
- The 49 expand fixtures and `tests/cli.rs`'s `PAGE_PROGRAM` that use `(doc ...)` for its string switch to `(doc-page ...)`.

## Decision

The project owner chose option A on 2026-11-04, with these answers:

1. **Option A:** `doc` prints the page as `rslisp doc` prints it and returns `unit`.
2. **The string lookup** is named `doc-page`. It returns the page as a string literal, as `doc` returns it today.
3. **The stream:** `doc` prints to standard output, through `println`, as Clojure's `doc` does.
4. **The header's color:** the header prints plain at the prompt, because the page is program output there. Only `rslisp doc` colors it, as RFC 0050 decision 7 says.
5. **No separator:** `doc` prints no line before the page, so the prompt and `rslisp doc` print the same text.

The owner also saw on 2026-11-04 how `doc` behaves in a `def`, and accepted it: `(def p (doc f))` binds `unit`, the page prints when a form first reads `p`, and again on each prompt input that reads it, and `(def p (doc-page f))` keeps the text.

## Rejected options and why

- **B: a `page` type:** a clean prompt needs a `show` that writes raw text, which breaks RFC 0048's readable one-line form, the `value` of `eval --json` and RFC 0050's value colors for one type. A lawful `show` prints `(->page :text "...")`, which is worse than today.
- **C: unquoted string results:** a string result no longer looks like a string or reads back, and the rule is an exception in Rust keyed on two types.
- **D: a second printing name:** `(doc square)`, the form a user types first, keeps the escaped string, and the page gets two names.
- **E: the prompt treats `doc` by name:** E makes the prompt evaluate one input differently by its head, so the prompt depends on how the prelude builds the page, which `AGENTS.md` forbids.
- **A separator line, as Clojure's dashes:** the prompt and `rslisp doc` would print two texts for one page.
- **Standard error:** `doc` is a `println` of the page, and a page is the output the user asked for, not a diagnostic.

## Rule

[docs/language.md: Docstrings](../docs/language.md#docstrings). The build that lands the decision states there that `(doc term)` prints the page to standard output and returns `unit`, that `(doc-page term)` returns the page as a string, and rewrites each sentence of that section that gives `(doc term)` a string value (lines 383, 389, 393, 398, 401, 410) to use `(doc-page term)`.
