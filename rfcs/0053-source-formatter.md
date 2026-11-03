# RFC 0053: A source formatter

**Status:** Accepted
**Date:** 2026-11-02

## Problem

The language has no formatter. `rustfmt` lays out the emitted Rust (D17), but nothing lays out the source. Every author indents by hand, a reviewer checks the indentation by eye, and an editor that formats on save has no command to call. RFC 0041 already chose the docstring position so that "a formatter lays both out alike", and RFC 0047 (open in #277) plans evaluation from Neovim, where format on save is expected.

A formatter for this language has four constraints that a formatter for Rust or Go does not have:

- **The reader is lossy.** `Reader::read` drops comments and blank lines, and `FormKind::String` holds the decoded value, not the text as written. Printing a `Form` back loses the comments and turns `"\u{e9}"` into `"é"`.
- **Layout depends on meaning.** `(defn twice (x)` puts its body 2 columns in, and `(str "a"` aligns its next argument under `"a"`. Which layout a list takes depends on what its head is. The principles forbid a Rust table of prelude names: "NEVER restate in Rust a name or a fact that the prelude states".
- **User macros are body forms too.** `macro_heavy` defines `unless-zero` and `dotimes` and lays out their bodies like `defn`. A table of known names cannot cover them.
- **The file may not compile.** An editor formats a file that the user is still writing. Meaning comes from expansion, and a file that fails to expand still needs a layout.

One fact makes the problem smaller than in most Lisps: the reader treats only `(`, `)`, `"`, `;` and whitespace specially. There is no quote shorthand and no reader macro, so a formatter lays out only lists, atoms, strings and comments.

## Measurements

A prototype indenter scored three indentation rules against the code in this repository. It read every tracked `.rslisp` file except the `tests/fixtures/read/` fixtures, which hold reader errors, and the two generated `deep_nesting` entries. That is 1,606 files and 13,174 continuation lines, where a continuation line is a line whose first token sits inside a list. A line matches a rule when its indentation is the one the rule gives. The prototype lives outside the repository and is not part of this RFC.

The three rules:

- **Uniform:** a list whose head is a symbol indents every continuation line 2 columns past its `(`. Any other list indents 1 column.
- **Align:** a continuation line aligns under the first argument when that argument is on the head's line, and indents 2 columns otherwise. A list whose head is not a symbol aligns under the head.
- **Block:** a form with a header of `n` forms, such as `defn` with `name parameters`, indents its continuation lines 2 columns when the head's line holds at most `n` forms after the head. Every other list follows the align rule. The prototype used these header lengths: `defn` 2, `defmacro` 2, `impl` 2, `rust-trait` 2, `let-pair` 2, `display` 2, `def` 1, `fn` 1, `macro` 1, `match` 1, `defstruct` 1, `defrule` 1, `enum` 1, `trait` 1, `loop` 1, `let-try` 1, `case` 1, `template` 0 and `quote` 0. A header of 0 needs no shape, because the align rule already indents 2 when nothing follows the head on its line: dropping `template` and `quote` from the list changes no score.

| Files | Continuation lines | Uniform | Align | Block |
| --- | ---: | ---: | ---: | ---: |
| All 1,606 | 13,174 | 84.0% | 16.0% | 96.0% |
| `prelude/` | 1,413 | 56.6% | 42.9% | 97.5% |
| `tests/fixtures/corpus/` | 3,891 | 68.5% | 28.4% | 93.4% |
| `tests/fixtures/run/` | 3,406 | 95.5% | 5.8% | 96.8% |
| `tests/fixtures/check/` | 1,298 | 98.3% | 3.9% | 97.0% |

Five facts follow:

- **The code already follows the block rule.** 96.0% of continuation lines match it. A block formatter would change 532 lines in the whole repository and 35 in the prelude. Only `tests/fixtures/check/` leans the other way, 98.3% uniform against 97.0% block, because its short fixtures hold most of the `if` forms that indent 2.
- **`if` aligns its branches.** 1,334 `if` continuation lines align under the test, and 126, mostly in `tests/fixtures/check/`, indent 2. The uniform rule would move all 1,334.
- **Most of the remaining lines are hand-written slips or user macros.** 143 are `str` and `vec` arguments in `literal_inference` and `generic_types` that align under nothing. 85 are the bodies of `unless-zero`, `dotimes` and `let-fields`, user macros that no table of prelude names covers. 24 are `use` lists, which align their names under the first name, not under the module path.
- **Spacing is already uniform.** No file has two blank lines in a row, a space after `(`, two spaces between tokens, a `)` alone on its line, trailing whitespace or a missing final newline.
- **Comments are almost absent.** The 39,770 tracked `.rslisp` lines hold one comment, because AGENTS.md treats a comment as a failure. User programs will hold more, and a legal header and a TODO are allowed, so the formatter must keep them.

Line length: 1,467 of the 27,552 lines in the measured files are longer than 80 columns, and 411 are longer than 100, the `max_width` of the repository's `rustfmt.toml`. 39 of the 411 are in the prelude.

## Options

The design has three independent choices: what the formatter changes, how it learns a form's layout, and where it runs. A fourth choice, where the kernel forms' shapes live, applies under 2B and 2C. Each choice lists its options.

### Choice 1: what the formatter changes

#### 1A: indentation and spacing, line breaks kept

The formatter keeps every line break that the author wrote between two forms, as `gofmt` and `cljfmt` do. It changes only:

- the indentation of each continuation line, by the layout rule of Choice 2;
- the space between two forms on one line, to one space;
- closing parentheses, which join the line of the last form before them;
- blank lines, to at most one in a row, none right after a `(` and none right before a `)`;
- the end of the file, to exactly one newline.

```lisp
(defn describe (n)
      (if (< n 0)
        "negative"
        (str "count "
          n   )))
```

```lisp
(defn describe (n)
  (if (< n 0)
      "negative"
      (str "count "
           n)))
```

Every output is a fixed point, so formatting twice gives the same text. Two layouts of the same forms can both be fixed points, as `(str "a" "b")` on one line and on two lines are. The formatter never breaks a long line and never joins two lines.

#### 1B: a canonical layout within a width

The formatter ignores the author's line breaks and lays each form out within a width, such as 100 columns, with a Wadler-style printer, as `rustfmt`, Black and zprint do. The output depends only on the forms and the comments, so two authors always get the same text.

The same input gives, at width 80:

```lisp
(defn describe (n)
  (if (< n 0) "negative" (str "count " n)))
```

The costs:

- Every form needs a rule for where it breaks: which pairs of a `defstruct` or a keyword call stay together, how `match` clauses break, how a `cond` lays out its test and body pairs. Each rule is a fact about a prelude form that the prelude would have to state.
- It rewrites data that the author laid out on purpose. `warehouse` builds `stock-text` from 30 string parts, one record per line. A width printer packs them as many to a line as fit.
- A comment between two forms decides where a line must break, so the printer needs a rule for each comment position.
- The emitted Rust follows `rustfmt`, but this language's users read the source, not the Rust, so the source keeps no layout it can borrow from `rustfmt`.

It also has one gain: `rslisp expand` prints each top-level form on one line today, and a width printer could lay out forms that no author wrote, such as an expansion or a `doc` header.

### Choice 2: how the formatter learns a form's layout

Each option shows how it lays out the same input:

```lisp
(defrule unless-zero
  ((value body ...) (unless (= value 0) body ...)))

(defn report (hits)
  (unless-zero hits
    (println (str "hits: "
                  hits))))
```

#### 2A: one uniform rule

The uniform rule above, as Nikita Prokopov proposed for Clojure: a list whose head is a symbol indents 2, and any other list indents 1. The formatter needs no fact about any form, so it works on a file that does not expand, and it restates nothing from the prelude.

```lisp
(defn report (hits)
  (unless-zero hits
    (println (str "hits: "
               hits))))
```

The cost is every aligned argument: an `if` lays out its test and its branches as peers, and `str` and `vec` arguments no longer line up. It matches 84.0% of the current lines and moves 2,113.

#### 2B: the block rule, with shapes read by name

The block rule above. A form's header length comes from the `header-shape` that its definer already states for `doc` (RFC 0044). `(header-shape (defn name parameters))` says that the header of a `defn` call is its name and its parameters: `doc` shows those forms, and the formatter keeps them on the head's line and indents the rest 2. One statement serves both, so a definer states its header once.

```lisp
(defrule unless-zero
  ((value body ...) (unless (= value 0) body ...)))

(header-shape (unless-zero value))
```

```lisp
(defn report (hits)
  (unless-zero hits
    (println (str "hits: "
                  hits))))
```

Without the `header-shape` line, `unless-zero` takes the align rule and its body aligns under `hits`.

The formatter reads shapes without expanding the file:

1. The kernel forms' shapes, from the place that Choice 4 picks.
2. The prelude's shapes, from the `header-shapes` notes in the embedded prelude state.
3. The `header-shape` forms written at the top level of the file and of the modules it `use`s, found by reading those files.

A list whose head has a shape takes the block rule. Every other list takes the align rule. The formatter matches a head by its name, so a local that shadows `match` still lays out as `match`. A wrong shape changes only the layout, never the meaning, so this costs no correctness.

`header-shape` today states "the header of a definer's calls". 2B widens it to the header of any macro's calls, such as `(header-shape (loop bindings))`. `doc` reads shapes only for definers, so the wider meaning changes no page.

#### 2C: the block rule, with shapes resolved by expansion

The formatter expands the file, resolves each list's head to its binding, and reads the shape from the defining module's notes, as `doc` does. Shadowing and renamed imports resolve as the compiler resolves them.

The costs:

- A file that fails to expand gets no layout, or a layout that changes when an unrelated error elsewhere in the file is fixed.
- Code inside `template` and `quote` is data with no bindings, and the prelude's macros are mostly templates, as in `(template (def (unquote name) (fn (unquote parameters) (splice body))))`. 2C must fall back to names inside a template, so it is 2B plus an expansion.
- Each format pays an expansion, about 11 ms for the 38-definition session that RFC 0047 measured, where 2B pays only a read.

### Choice 3: where the formatter runs

The macros of the prelude cannot format a file. A macro sees forms without comments, without blank lines and without a string's written text, and it runs only on a file that reads and expands. The layout facts, the shapes, stay in the prelude and the kernel, and the formatter that applies them is Rust.

#### 3A: a front-end stage over a lossless syntax tree

A new stage under `expand/src/format/` takes a `SourceFile` and returns the formatted text or the reader's `Diagnostics`. It depends on the reader and on the shapes that Choice 2 and Choice 4 name. It reads the file twice: once with `Reader::read`, so a file with a reader error reports the same `L01xx` diagnostics as `rslisp check` and is not formatted, and once into a syntax tree that keeps comments, blank lines and each atom's text as written. The syntax tree reader reuses `Cursor`, `Syntax` and `StringLiteralReader`, so the two readers share every rule of the syntax. A compile never builds the syntax tree, so it pays nothing.

`rslisp fmt` lives in `src/cli/`, which reads and writes the files.

#### 3B: trivia in the compiler's reader

`Reader` keeps comments and blank lines beside each `Form`, and `Form` keeps the written text of a string. The formatter then needs no second reader. Every compile pays for trivia that only the formatter reads, and `Form` grows for every stage. `Form` is `Rc`-shared and cloned by the expander, so the cost reaches every expansion.

### Choice 4: where kernel shapes live

Most multi-line forms in the repository are kernel forms, and the kernel has no source. `create_builtin` in `expand/src/expand/module_loading.rs` builds the `kernel` module from `KernelForm::ROWS`, so no `header-shape` can be written in the module that defines `fn` or `match`. Under 2B or 2C, the kernel's shapes need a home of their own.

The prototype measured each kernel form's layout over the same 13,174 lines. A body layout indents 2 past the `(`, and an aligned layout lines up under the first argument.

| Kernel form | Continuation lines | Layout | Header length |
| --- | ---: | --- | ---: |
| `match` | 756 | body | 1 |
| `impl` | 510 | body | 2 |
| `fn` | 452 | body, except 8 slips | 1 |
| `def` | 365 | body | 1 |
| `trait` | 242 | body | 1 |
| `enum` | 186 | body | 1 |
| `rust-trait` | 125 | body | 2 |
| `macro` | 63 | body | 1 |
| `let` | 18 | body | 1 |
| `type`, `new`, `rust-type` | 4 each | body | 1 |
| `if` | 1,460 | 1,334 aligned, 126 body | none |
| `do` | 116 | aligned, or 2 when nothing follows the head | none |
| `export` | 35 | aligned | none |
| `use` | 24 | names under the first name | 1, by open question 3 |
| `rust` | 11 | aligned | none |
| `recur` | 2 | aligned | none |

Three facts follow:

- **12 kernel forms need a shape**, each with a header of 1 or 2. The other kernel forms either align or never span two lines in the repository.
- **Without them the block rule loses 20 points.** It matches 75.6% of the lines instead of 96.0%, and 2,688 more lines move. `(def name` followed by its value on the next line would align the value under `name`, 5 columns in.
- **`if`, `do`, `export`, `rust` and `recur` need none.** The align rule already gives their layout.

The same twelve shapes, written as `header-shape` would state them:

```lisp
(header-shape (def name))
(header-shape (let name))
(header-shape (type name))
(header-shape (fn parameters))
(header-shape (macro parameters))
(header-shape (match value))
(header-shape (new name))
(header-shape (enum name))
(header-shape (trait name))
(header-shape (rust-type name))
(header-shape (rust-trait name path))
(header-shape (impl trait type))
```

#### 4A: a header length in the kernel table

Each `KernelFormRow` gains a header length beside its name and the shape that its diagnostics show:

```rust
KernelFormRow {
    form: KernelForm::Expression(ExpressionForm::Fn),
    name: "fn",
    shape: "(fn (param ...) body ...)",
    header: 1,
},
```

A header of 0 means the align rule, so `if` and `do` state 0 and need no special value. The formatter reads `KernelForm::all()` and matches a head by the kernel name, which the prelude re-exports unchanged.

The gains:

- The compiler owns the kernel, so the row restates no fact that the prelude states. The row already says where a form's body goes, in the help text of its usage shape.
- No program pays for it. The table is a constant, and the saved prelude state does not grow.
- A shape cannot name a form that does not exist, because each header sits in its form's row.

The costs:

- Shapes come from two places: Rust for the kernel and `header-shape` for every macro. The formatter merges them, and `docs/language.md` states both.
- The layout of `fn` is stated nowhere in the language. A user who reads the prelude to learn why `match` indents its clauses 2 finds no statement.
- A layout fact enters the compiler, whose job AGENTS.md limits to reading, checking, reporting and emitting.

#### 4B: `header-shape` statements beside the prelude's re-export

`prelude/prelude.rslisp` already names every kernel form in its `export`, so the kernel's shapes sit beside that list. The module adds `header-shape` to its `use` of `prelude/macros` and states the twelve shapes above.

The gains:

- Every shape is a `header-shape` form, for a kernel form, a prelude macro or a user macro. The formatter has one source and one rule for it.
- The compiler gains nothing. "If the prelude can express a feature, NEVER add a kernel form for it" and "Every other feature belongs in the prelude" both point here.
- `doc` does not change. A lookup reads the shapes of the module that defines the definer, and a kernel form never defines a binding through a macro call, so `definition-call` never returns one and no page reads a kernel shape.

The costs:

- The statements live outside the module that defines the forms. `docs/language.md` says to state a shape in the module that defines the definer, because that is where `doc` looks. Under 2B the formatter already reads shapes by name wherever they are written, so 4B adds no rule that 2B does not already need, but `docs/language.md` must state both readers.
- Every program loads twelve more notes. `header-shape` adds each shape to `prelude/prelude`'s notes, which the saved prelude state holds, and every run deserializes that state. Each note is a list of two or three symbols, so the rise should be far below the 1% that the counters gate allows on `empty_program`, but the build must measure it with `nix develop -c cargo xtask bench measure`.
- `header-shape` checks only that its argument is a list that starts with a symbol, so `(header-shape (mtach value))` lays out nothing and reports nothing. The same is true of a user's shape. A check that the name is bound, reported as `L0201`, would catch both.

#### 4C: kernel notes that the compiler writes

`create_builtin` adds a `header-shapes` note to the `kernel` module for each form, so the formatter and `doc` read a kernel shape exactly as they read a macro's. Rust would then write the note key `header-shapes` and the note's layout, which the prelude's `header-shape` macro chose, and "NEVER restate in Rust a name or a fact that the prelude states" forbids that.

#### 4D: headers derived from the kernel's usage shapes

The formatter parses the `shape` text of each row and counts the positions before the first repeated one. That gives the right header for `fn`, `match`, `enum`, `trait`, `impl` and `rust-trait`, and fails elsewhere:

- `(if test then else)` and `(def name value)` repeat nothing. A rule that gives `def` a header of 1 gives `if` a header of 2, which moves 1,334 aligned lines.
- The shape of `macro` is `(def name (macro (form) body ...))`, which describes the `def` around it.
- The shapes are help text for the "write it as" line of a malformed form's diagnostic. Rewording a help line would move code.

#### 4E: no kernel shapes

Every kernel form takes the align rule. It moves 2,688 lines that 2B keeps, as the facts above show.

## Recommendation

1B, 2B, 3A and 4B. The project owner decided all four.

- **1B gives every program one layout.** The formatter owns every line break, so the same forms always give the same text, as with `rustfmt`, Black and zprint. The costs in 1B's section are the design work that open questions 6 to 9 start.
- **2B matches the code as it is written.** It matches 96.0% of the current lines, against 84.0% for 2A. It reads every layout fact from a statement that the prelude or the kernel already makes, so the Rust names no prelude form. It works on any file that reads, and a user macro gets its layout from one `header-shape` line beside its definition.
- **3A keeps the compile path as it is.** The formatter reports reader errors with the reader's own diagnostics, and a compile builds nothing for it.
- **4B gives every shape one form.** The formatter reads kernel, prelude and user shapes through `header-shape`, and the compiler gains no layout fact.

Under 1B and 2B, the formatter lays out each list by these rules. Rules 3 to 6 place the lines of a list that breaks, and the 96.0% match of the block rule in Measurements is the evidence for them.

1. A top-level form starts at column 0.
2. A list stays on one line when it is at most 40 columns long, from its `(` to its `)`, and fits within the 80-column width on the rest of its line. A `defn` stays on one line only when its body is also one form.
3. A list that does not fit breaks. A list whose head has a shape of `n` header forms keeps the head and its header on the first line, and puts each later form on its own line, 2 columns past its `(`.
4. A `use` that does not fit keeps its module path and its first name on the first line, and puts each later name on its own line, under the first name, as rule 5 does for the arguments of a call.
5. Any other list whose head is a symbol keeps its first argument on the head's line and puts each later argument on its own line, under the first argument. When that layout takes more lines than putting every argument on the lines below the head, 2 columns past the `(`, or does not fit at all, the list takes the second layout, as `raco fmt` does.
6. A list whose head is not a symbol, such as a parameter list or a `match` clause, puts each later form on its own line, under its head.
7. In a list that breaks, a keyword and the form after it share one line, as a pair, wherever rules 3 to 6 put a form on its own line. A keyword is a symbol that starts with the marker that `Keyword::MARKER` states, which the compiler already owns, so the formatter restates no prelude fact.
8. A trailing comment ends its line, so the form after it starts a new line.
9. A string keeps its text, except a docstring. A line that starts inside a multi-line string is part of the string, and the formatter leaves it as written. A docstring's continuation lines move by as many columns as its opening line moves, so they keep their place under the body.

The formatter checks two properties on every file before it writes it, and writes nothing if either fails:

- **It keeps the meaning.** Reading the output gives the same forms as reading the input, compared without spans. Two docstrings compare equal when they are equal after removing the indentation that their continuation lines share, as a lookup removes it, so moving a docstring's lines passes the check.
- **It is stable.** Formatting the output again gives the same text.

A failure is a formatter bug, so the command reports it as an internal error with the file's name and leaves the file as it was.

### Details

- **Command:** `rslisp fmt <path>...` rewrites each file in place, and each directory's `.rslisp` files, the extension that `Language::EXTENSION` states, recursively. It prints nothing when it succeeds.
- **Check mode:** `rslisp fmt --check <path>...` writes nothing, prints the path of each file that would change, and exits 1 if any would.
- **Editors:** `rslisp fmt -` reads standard input and writes the formatted text to standard output, so Neovim's `formatprg` and `conform.nvim` call it with no plugin of the project's own.
- **Exit codes:** 0 when every file is formatted or already was. 1 when a file has a reader error, or would change under `--check`. 2 for a usage error. These are the codes that `ExitStatus` already gives.
- **Configuration:** none. The indentation is 2 columns, the width is 80 columns (open question 6), and the rules have no options, so every program looks alike, as with `gofmt`.
- **Blank lines:** the formatter keeps a blank line that the author wrote between two forms, at most one in a row, as `rustfmt` and Black do. It removes a blank line right after a `(` or right before a `)`.
- **Strings:** the formatter never changes a string's text, escapes included, except the indentation of a docstring's continuation lines. Numbers keep their text too, so `1_000` stays `1_000`.
- **Own-line comments:** a comment on its own line takes the indentation of the form that follows it in the same list, or the indentation that a next form would take when none follows.
- **Trailing comments:** a comment after a form stays at the end of its line, one space after the form, as `raco fmt` does. A `)` never takes a line of its own: a `)` that would follow a trailing comment moves before it, so `b ; why` followed by `)` becomes `b) ; why`. The comment still ends the same line, so the meaning does not change.
- **The repository:** a CI check runs `rslisp fmt --check` over `prelude/`, `examples/` and `tests/fixtures/`, except the `read` fixtures and fixtures whose layout is their subject. The commit that adds the check formats those files, so AGENTS.md's indentation then rests on an automated check, not on prose.
- **Tests:** fixtures under `tests/fixtures/format/` record the input and the output in an insta snapshot. A test formats every `.rslisp` file in the repository and asserts both properties above. `tests/cli.rs` covers the exit codes, `--check` and standard input.

### Open questions

1. **Where kernel shapes live.** Decided: in the prelude, as 4B, and `header-shape` reports a name that is not bound as `L0201`. See Decision.
2. **How the formatter finds a user's shapes.** Decided: follow `use`. See Decision. A user states `(header-shape (unless-zero value))` beside the macro, often in a different module from the files that call it, and the formatter sees the shape only if it reads that module. `use` cannot rename a name, so `header-shape` and the macro's name are spelled the same in every file, and matching by name finds both. Two cases still escape it: a shape that a macro writes, which only expansion shows, and a module the formatter cannot locate, because a `use` path resolves against the entry file's directory and the formatter is given one file, not the entry. A form whose shape is not found lines up like a call. The options:
   - **The file alone.** The formatter reads the prelude's shapes and the file's own. A macro from another module lines up like a call unless each calling file restates its shape.
   - **Follow `use` (chosen).** The formatter reads every module that the file reaches through `use`, and tries each path against the file's directory, then each parent directory, until the module exists. `rslisp fmt -` takes the file's path as an argument, so an editor gets the same layout as the command line.
   - **The whole directory.** The formatter reads the shapes of every `.rslisp` file under the directory that `rslisp fmt` is given. It needs no path lookup, but a file's layout then depends on which directory the command was given, and an editor that formats one file sees no other.
3. **`use` lists.** Decided: names line up under the first name, as today. See Decision.
4. **Trailing comments.** Decided: one space before each, and a `)` moves before a trailing comment instead of taking its own line. See Decision.
5. **Docstring continuation lines.** Decided: they move with their opening line, and a definer marks that it takes a docstring with `:docstring` on its `header-shape`. See Decision.
6. **The width.** Decided: 80 columns. See Decision.
7. **Short forms.** Decided: a list stays on one line only within 40 columns, and a `defn` only when its body is one form. See Decision.
8. **One per line or fill.** Decided: one item per line. A form either fits on one line or puts one item per line, so nothing fills. See Decision.
9. **Pairs.** Decided: a keyword and its value share a line. See Decision.

## Decision

The project owner decided on 2026-11-03 that the kernel forms' shapes live in the prelude (Choice 4B): `prelude/prelude.rslisp` states the twelve shapes with `header-shape`, beside its re-export of the kernel. The build measures the cost of the twelve notes on `empty_program` with `nix develop -c cargo xtask bench measure`. Open question 1 also asked whether `header-shape` should report a name that is not bound; the owner's answer follows below.

The project owner decided on 2026-11-03 that `header-shape` reports a form name that is not bound as `L0201` (open question 1):

```text
error[L0201]: cannot find `unles-zero` in this scope
 --> report.rslisp:2:16
  |
2 | (header-shape (unles-zero value))
  |                ^^^^^^^^^^ not found in this scope
  |
help: a name with a similar spelling exists
  |
2 - (header-shape (unles-zero value))
2 + (header-shape (unless-zero value))
  |
```

When a bound name is close in spelling, the error suggests it, with the same help that `L0201` already gives for an unknown name, such as `(doc defm)` suggesting `def`. Without the check, a misspelled shape applies to no form and the formatter lays the intended form out as a call, with nothing to say why. `header-shape` resolves its name in the prelude, the way `doc` already reports `L0201` for an unknown term, so the compiler gains no code. `rslisp fmt` reads shapes without expanding, so the error shows on `rslisp check`, `build` and `run`. The prelude writes each shape after its definer, and the build tests whether a shape written above its definer resolves; if it does not, the error's help asks to move the shape below the definition. The prelude's shapes resolve once, when the compiler builds the prelude state.

The project owner decided on 2026-11-03 that the formatter finds a user's shapes by following `use` (open question 2). It reads every module that the file reaches through `use`, tries each path against the file's directory and then each parent directory, and `rslisp fmt -` takes the file's path so an editor gets the same layout as the command line. A form whose shape it cannot find lines up like a call.

The project owner decided on 2026-11-03 that a `use` list keeps its names under the first name (open question 3):

```lisp
(use code first rest list
          concat string)
```

Every `use` is `(use module name ...)`: one module path, then bare names, as all 562 `use` forms in the repository are. `prelude/prelude.rslisp` states a thirteenth kernel shape, `(header-shape (use module))`, and a shaped form whose first line runs past its header aligns its later lines under the first form after the header. The prototype shows that this brings 21 of the 24 `use` lines in line with the current style and changes the layout of no other form in the repository. `export` needs no shape, because a call already aligns its arguments under the first one. Under 1B, layout rule 4 states this for `use`.

The project owner decided on 2026-11-03 that the formatter owns every line break (Choice 1B), as most formatters do. A list stays on one line when it fits within the width and breaks when it does not, so the author's line breaks no longer decide the layout. A `use` breaks only when it does not fit.

The project owner decided on 2026-11-03 that the width is 80 columns (open question 6). It matches the Clojure community style guide and zprint's default, and the docstring rule in `docs/language.md`, so code and docstrings share one width. The other widths weighed:

- **100 columns:** the width of the Google Common Lisp Style Guide and of the repository's `rustfmt.toml`, which lays out the emitted Rust.
- **102 columns:** the width of the Racket style guide and `raco fmt`.

At 80 columns the formatter breaks 1,467 lines of the measured files that are longer than 80, against 411 at 100.

The project owner decided on 2026-11-03 that a docstring's continuation lines move with its opening line (open question 5). When the formatter re-indents a definition, it shifts each continuation line of the docstring by as many columns as the opening line moves:

```lisp
(defn twice (x)
  "Doubles a number.

  Works on any number."
  (* 2 x))
```

A lookup removes the indentation that the continuation lines share, so the page does not change, and the meaning check compares docstrings after that removal. The formatter learns which string is a docstring from the definer. Every definer that takes a docstring takes it as the first form after its header (RFC 0041), so the owner decided that a definer marks it on its `header-shape`:

```lisp
(header-shape (defn name parameters) :docstring)
(header-shape (defmacro name pattern) :docstring)
(header-shape (defstruct head) :docstring)
(header-shape (defrule name) :docstring)
```

For a marked definer, a string right after the header is a docstring when another form follows it, as RFC 0041 decided for every definer, so `(defn greeting () "hi")` keeps its string as written. `doc` ignores the marker, because it reads only the header. A separate statement, such as `(docstring-after-header defn)`, was rejected because it doubles the statements per definer. Guessing with no statement was rejected because a user macro may take a string as data right after its header, and shifting its lines would change what the program prints in a way the meaning check, which ignores docstring indentation, would not catch. zprint and `raco fmt` never change a string's text, so a re-indented definition can leave its docstring's continuation lines behind. The repository holds 6 multi-line strings, all docstrings.

The project owner decided on 2026-11-03 how trailing comments lay out (open question 4). A trailing comment sits one space after its form, as `raco fmt` does, so the formatter drops a gap that the author widened to line comments up. A `)` never takes a line of its own: one that would follow a trailing comment moves before it:

```lisp
(def limit 10) ; the most items
(def threshold 25) ; the reorder point

(+ a ; first operand
   b) ; second operand
```

`raco fmt` and zprint put such a `)` on a line of its own, and the owner rejected that layout. zprint keeps the author's gap before a comment, and `gofmt` lines up the trailing comments of consecutive lines into one column, which moves the comments of neighboring lines when one line changes.

The project owner decided on 2026-11-03 to apply the 40-column limit to every list, at every depth (open question 7). A form longer than 40 columns breaks even when it fits in 80:

```lisp
(defstruct point :x f64 :y f64)

(defstruct parcel-36
  :serial u32
  :mass f64
  :sealed bool)

(enum token
  (symbol (ref str))
  (number (ref str)))

(defn main ()
  (println (str "distance: "
                (distance (->point 0 0) (->point 3 4)))))

(use warehouse/model item
                     entry
                     valued
                     value-cents
                     category
                     dollars
                     widen)
```

The owner first exempted `use` and `export`, then applied the limit to them too, so the rule has no exception. The rule names no form, so the prelude states nothing for it. It replaces the earlier choice of a 40-column limit for `defn` alone. Of the 4,517 one-line top-level forms in the measured files, 2,465 are longer than 40 columns and break, 537 of them `use` and `export` forms.

The project owner decided on 2026-11-03 that a call whose aligned arguments would take more lines moves every argument below its head instead, as `raco fmt` does (layout rule 5):

```lisp
(for-each
  (entries totals)
  (fn (entry)
    (let-pair (name worth) entry
      (println (str "  " name ": " (dollars (deref worth)))))))
```

Aligned under `(entries totals)`, the `println` line would reach 82 columns and need one more break. `raco fmt` puts the arguments 1 column past the `(`, under the head's name, as Racket does. This formatter puts them 2 columns past it, as the repository does for a list whose head stands alone on its line. Choosing between the two layouts means comparing them, so the printer searches for the layout with the fewest lines within the width, as `raco fmt` does with its pretty-expressive printer.

The project owner decided on 2026-11-03 that a keyword and the form after it stay together on one line when their list breaks (open question 9):

```lisp
(->reading :site label
           :level base
           :limit 40)

(defstruct reading
  :site str
  :level i64
  :limit i64)
```

The project owner decided on 2026-11-03 that a `use` that does not fit in 80 columns puts one name per line, under the first name, in place of filling each line (open question 8):

```lisp
(use warehouse/model item
                     entry
                     valued
                     value-cents)
```

Adding or removing a name then changes one line. `export` follows the same rule through rule 5, as zprint and `raco fmt` lay it out. Of the measured files, 404 `use` and `export` forms are longer than 80 columns, and they take 1,545 lines this way.

The project owner decided on 2026-11-03 that a `defn` stays on one line only when the whole form fits in 40 columns and its body is one form (open question 7). Any other `defn` puts its body on the next line, even when it fits in 80 columns:

```lisp
(defn sum (values) (fold values 0 add))

(defn rule-1-start-shift (seed)
  (+ seed (next-rule-1-start (rule-1-start))))

(defn change! (items)
  (push! items 1)
  (len items))
```

The owner first chose to break every `defn` body, then chose this rule after comparing Clojure's formatters. zprint and cljfmt keep a short `defn` on one line, and the Clojure community style guide shows `(defn foo [x] (bar x))` as good style. `rustfmt` and Black always break a function body. The 40-column limit follows `rustfmt`'s small heuristics, which keep some forms on one line only within a fraction of the full width, such as 50 of 100 columns for an `if` and `else`. Of the 1,182 one-line `defn` forms in the measured files, 649 meet both conditions and stay as they are.

The project owner decided on 2026-11-03 that the formatter reads shapes by name without expanding the file (Choice 2B), and that it is a front-end stage over a syntax tree of its own (Choice 3A). The formatter then lays out a file that reads but does not compile, as an editor's format on save needs, and a compile builds nothing for it. Every other decision above assumes both: kernel shapes in the prelude, shapes found by following `use`, the `L0201` check and the `:docstring` marker.

Comparing layouts, as layout rule 5 does when it moves a call's arguments below its head, needs a printer that searches for the layout with the fewest lines within the width, as `raco fmt` does with pretty-expressive. The build writes that printer or takes a library for it.

## Rejected options and why

The project owner chose 1B on 2026-11-03, which rejects 1A, and chose 4B, which rejects 4A, 4C, 4D and 4E, and chose to follow `use` for open question 2, which rejects reading the file alone and reading the whole directory, and chose a shape for `use` for open question 3, which rejects lining its names up under the module path, and chose 2B and 3A, which reject 2A, 2C and 3B.

- **2A: one uniform rule:** it indents every broken list 2 columns, so `if` branches and `use` names no longer line up under the first form, a layout the owner rejected.
- **2C: shapes resolved by expansion:** it cannot lay out a file that fails to expand, it falls back to names inside every template anyway, and it pays an expansion per format.
- **3B: trivia in the compiler's reader:** every compile pays for comments and written text that only the formatter reads.
- **1A: indentation and spacing, line breaks kept:** two authors get different text for the same forms, a long line stays long, and a `use` keeps its names on a second line wherever the author put them.
- **4A: a header length in the kernel table:** it splits shapes between Rust and the prelude and puts a layout fact in the compiler.
- **4C: kernel notes that the compiler writes:** Rust would restate the note key and layout that the prelude's `header-shape` chose.
- **4D: headers derived from the kernel's usage shapes:** it gives `if` a body layout, cannot read `macro`'s shape, and ties layout to diagnostic help text.
- **4E: no kernel shapes:** it moves 2,688 lines and puts a `def`'s value under its name.
- **Shapes from the file alone:** a macro from another module lines up like a call unless every calling file restates its shape.
- **Shapes from the whole directory:** a file's layout would depend on the directory the command was given, and an editor that formats one file would see no other.
- **`use` names under the module path:** it moves 24 lines of the prelude away from the layout every `use` has today.

## Rule

[docs/language.md: Source formatting](../docs/language.md#source-formatting). The build adds that section, which states the layout rule, the comment rules and the `rslisp fmt` command.
