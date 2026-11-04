# RFC 0050: Colors at the prompt

**Status:** Accepted
**Date:** 2026-11-04

## Problem

`rslisp repl`, pull request 299, prints plain text. The prompt, a typed input, a printed value, the program's output and the compiler's diagnostics look alike, so a user scans a session to find where one input's output ends and the next input starts. Line editing and history with `rustyline` follow pull request 299, and that pull request can color the input as the user types it.

The compiler already colors one thing: its diagnostics. Any colors the prompt adds must agree with them, so that `error` is the same red at the prompt, under `check`, and under `eval`.

Today's colors have three faults, which these probes show. They ran on a release build of `main` at `6091b050`, built with `nix develop -c cargo build --release`, on Linux, with `script -qec` to give the command a terminal:

| Probe | Result |
| --- | --- |
| `rslisp check bad.rslisp` on a terminal | colored: `\e[1m\e[91merror[L0401]\e[0m\e[1m: mismatched types` |
| the same with `NO_COLOR=1` | colored |
| the same with `TERM=dumb` | colored |
| the same piped, with `CLICOLOR_FORCE=1` | plain |
| `rslisp --help` on a terminal | colored: `\e[1m\e[4mUsage:\e[0m` |
| `rslisp --help` with `NO_COLOR=1`, or with `TERM=dumb` | plain |
| `rslisp --help` piped, with `CLICOLOR_FORCE=1` | colored |
| `rslisp eval m.rslisp '(iter (vec 1 2))'` on a terminal | the note is a colored diagnostic |
| `rslisp eval m.rslisp '(pair (f) "x")'` on a terminal | the value prints plain |
| `rslisp doc vec` on a terminal | the page prints plain |

1. **The diagnostics ignore `NO_COLOR`, `TERM=dumb` and `CLICOLOR_FORCE`.** `Terminal::renderer` in `src/cli/terminal.rs` picks `DiagnosticRenderer::styled` when standard error is a terminal and `DiagnosticRenderer::plain` otherwise, and checks nothing else.
2. **Two parts of one command decide color by two rules.** clap's help goes through `anstream`, which honors those variables, so `NO_COLOR=1 rslisp check` prints plain help and colored errors.
3. **A user cannot force color or turn it off.** No flag exists, and the variables do not reach the diagnostics.

### What `rslisp` colors today

`DiagnosticRenderer` in `expand/src/diagnostic/render.rs` wraps `annotate-snippets` 0.12, and `DiagnosticRenderer::styled` uses `Renderer::styled()` with that crate's default styles ([annotate-snippets 0.12.17, `src/renderer/mod.rs`](https://github.com/rust-lang/annotate-snippets-rs/blob/0.12.17/src/renderer/mod.rs)). On Linux and macOS they are:

| Part | Style | Escape |
| --- | --- | --- |
| `error`, its carets and label | bold bright red | `1;91` |
| `warning`, its carets and label | bold yellow | `1;33` |
| `note` | bold bright green | `1;92` |
| `help` | bold bright cyan | `1;96` |
| line numbers, `-->`, `\|`, `=`, secondary carets | bold bright blue | `1;94` |
| the title after the level | bold | `1` |
| a suggestion's added text | bright green | `92` |
| a suggestion's removed text | bright red | `91` |

These are the colors of rustc's diagnostics, and every one of them is a 16-color ANSI code. Every command that reports a diagnostic uses them through `Terminal::report` and `Terminal::print_diagnostics`: `check`, `signatures`, `doc`, `expand`, `emit`, `build`, `run`, `eval` and, in pull request 299, `repl`. Every other line the CLI writes, except clap's help and usage errors, such as `error: cannot read ...`, the `note: the program exited with code N` of pull request 299, a printed value and a `doc` page, is plain.

## Prior art

Each row is the REPL's default with no user configuration. A cell marked *unconfirmed* holds a claim that neither documentation nor source code confirmed. The sources follow the table.

| REPL | Prompt | Input as typed | Result value | Program output | Errors and traces | Warnings, notes | Color turns off with | User theme |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| IPython 9 | `In [`, `]: ` green, number bold bright green; `Out[` red; continuation green | yes: strings yellow, numbers green, functions blue; keywords and comments from pygments *(hex, unconfirmed)* | plain | plain | exception name red, file and line bright green, failing range black on yellow | *unconfirmed* | `--colors=nocolor`; stream not a terminal gives a simple prompt; `NO_COLOR` and `TERM=dumb` reach prompt_toolkit only | yes, JSON themes |
| Python 3.13 | `>>>`, `...` bold magenta | no | plain | plain | type bold magenta, message magenta, file and line magenta, failing span bold red | plain | `PYTHON_COLORS=0`, `NO_COLOR`, `TERM=dumb`, not a terminal; `FORCE_COLOR` forces | no |
| Python 3.14 | bold magenta, continuation too | yes: keywords bold blue, builtins cyan, strings green, numbers yellow, comments red, definitions bold | plain *(unconfirmed)* | plain | as 3.13, plus error range red | *unconfirmed* | as 3.13 | private `_colorize.set_theme` |
| Node.js | `> `, `\| ` plain | yes on `main` *(first release unconfirmed)*: keywords cyan, numbers and booleans yellow, strings green, comments grey | `util.inspect`: numbers and booleans yellow, strings and symbols green, `null` bold, `undefined` grey, dates magenta | plain | `Uncaught` plain; internal stack frames grey | plain *(unconfirmed)* | `NO_COLOR`, `NODE_DISABLE_COLORS`, `TERM=dumb`, not a terminal; `FORCE_COLOR` forces | yes, `util.inspect.styles` |
| Deno | `> ` plain | yes: strings green, numbers and literals yellow, keywords cyan, comments grey, called names bright blue | colored by Deno's inspect *(per-type colors unconfirmed)* | plain | `error:` red | startup warning yellow | `NO_COLOR`; `FORCE_COLOR` forces; terminal check *unconfirmed* | no |
| Ruby `irb` | plain | yes: keywords green, integers bold blue, strings red, symbols yellow, `nil` and `true` bold cyan | re-highlighted with the input table, for core literal types only | plain | Ruby's bold and underline message highlight | plain | `NO_COLOR`, `TERM=dumb` or unset, not a terminal, `--nocolorize` | no |
| Elixir IEx | `iex(1)>`, `...(1)>` plain | no *(unconfirmed)* | yellow base; atoms cyan, numbers yellow, strings green, booleans and `nil` magenta | plain | banner and stack trace red | interrupt yellow | `--no-color`, `colors: [enabled: false]`, not a terminal; `NO_COLOR` *unconfirmed* | yes, `IEx.configure` |
| Julia | `julia>` green, `help?>` yellow, `shell>` red, `pkg>` blue; no continuation prompt | no | plain | plain | `ERROR:` bold light red, frames bold | `@warn` yellow, `@info` cyan | `--color=no`, `TERM=dumb` gives the basic REPL; `NO_COLOR` *unconfirmed* | yes, `JULIA_*_COLOR` |
| GHCi | `ghci>` plain *(unconfirmed)* | no | plain | plain | `error` bold red, margin bold blue | `warning` bold magenta | `-fdiagnostics-color=never`; `auto` terminal check and `NO_COLOR` *unconfirmed* | yes, `GHC_COLORS` |
| Clojure `clj` | `user=>` plain | no; rlwrap flashes the matching paren | plain | plain | plain *(unconfirmed by source)* | plain | nothing to turn off | no |
| rebel-readline | `user=>` plain | yes, bold 256-color: strings 180, keywords 149, comments 243, core forms 39 | the input faces | plain | inline eval errors red | default | `:repl/toggle-color`; refuses a dumb terminal | yes, themes and `:color-theme` |
| Leiningen REPLy | `user=>`, `#_=>` plain | no | plain; blue with `--color` | plain; standard error red with `--color` | plain; red with `--color` | plain | off unless `--color` | no |
| Racket CS | *unconfirmed* | yes: literals green, identifiers light blue, comments yellow, parens red | *unconfirmed* | *unconfirmed* | *unconfirmed* | *unconfirmed* | `expeditor-color-enabled` | yes, `expeditor-set-syntax-color!` |
| Scala 3 | `scala>` magenta; continuation plain | yes: keywords yellow, definitions cyan, literals and types green, comments blue | colored through pprint *(per-kind colors unconfirmed)* | plain *(unconfirmed)* | the snippet is highlighted *(label colors unconfirmed)* | *unconfirmed* | `-color:never` | no |
| evcxr | `>> ` yellow | no | plain | standard error bright red | rustc errors as ariadne reports, header red, labels 256-color | not shown *(unconfirmed)* | no way found | no |

Sources:

- **IPython** 9.18.1: [`neutral-posix.json`](https://github.com/ipython/ipython/blob/9.18.1/IPython/utils/themes/neutral-posix.json), [`prompts.py`](https://github.com/ipython/ipython/blob/9.18.1/IPython/terminal/prompts.py), [`ultratb.py`](https://github.com/ipython/ipython/blob/9.18.1/IPython/core/ultratb.py#L500), [`displayhook.py`](https://github.com/ipython/ipython/blob/9.18.1/IPython/core/displayhook.py), [simple prompt when not a terminal](https://github.com/ipython/ipython/blob/9.18.1/IPython/terminal/interactiveshell.py#L116-L134), [`PyColorize.py`](https://github.com/ipython/ipython/blob/9.18.1/IPython/utils/PyColorize.py), prompt_toolkit's [`color_depth.py`](https://github.com/prompt-toolkit/python-prompt-toolkit/blob/main/src/prompt_toolkit/output/color_depth.py) and [`vt100.py`](https://github.com/prompt-toolkit/python-prompt-toolkit/blob/main/src/prompt_toolkit/output/vt100.py).
- **Python 3.13**: [`_pyrepl/reader.py`](https://github.com/python/cpython/blob/3.13/Lib/_pyrepl/reader.py#L494-L495), [`traceback.py`](https://github.com/python/cpython/blob/3.13/Lib/traceback.py#L187-L189), [`_colorize.py`](https://github.com/python/cpython/blob/3.13/Lib/_colorize.py).
- **Python 3.14**: [`_colorize.py` `Syntax` and `Traceback`](https://github.com/python/cpython/blob/3.14/Lib/_colorize.py#L157-L216), [`can_colorize`](https://github.com/python/cpython/blob/3.14/Lib/_colorize.py#L274-L315), [`set_theme`](https://github.com/python/cpython/blob/3.14/Lib/_colorize.py#L347-L353), [What's New in 3.14](https://docs.python.org/3.14/whatsnew/3.14.html).
- **Node.js**: [`lib/repl.js`](https://github.com/nodejs/node/blob/main/lib/repl.js), [`lib/internal/repl/highlight.js`](https://github.com/nodejs/node/blob/main/lib/internal/repl/highlight.js), [`inspect.styles`](https://github.com/nodejs/node/blob/main/lib/internal/util/inspect.js#L506), [`internal/util/colors.js`](https://github.com/nodejs/node/blob/main/lib/internal/util/colors.js), [`internal/tty.js`](https://github.com/nodejs/node/blob/main/lib/internal/tty.js), [Customizing `util.inspect` colors](https://nodejs.org/api/util.html#customizing-utilinspect-colors).
- **Deno**: [`cli/tools/repl/editor.rs`](https://github.com/denoland/deno/blob/main/cli/tools/repl/editor.rs), [`cli/tools/repl/session.rs`](https://github.com/denoland/deno/blob/main/cli/tools/repl/session.rs), [`cli/tools/repl/mod.rs`](https://github.com/denoland/deno/blob/main/cli/tools/repl/mod.rs), [`deno_terminal` `colors.rs`](https://github.com/denoland/deno_terminal/blob/main/src/colors.rs).
- **irb**: [`lib/irb/color.rb`](https://github.com/ruby/irb/blob/master/lib/irb/color.rb), [`lib/irb/init.rb`](https://github.com/ruby/irb/blob/master/lib/irb/init.rb), [`lib/irb.rb`](https://github.com/ruby/irb/blob/master/lib/irb.rb).
- **IEx**: [`lib/iex/lib/iex/config.ex`](https://github.com/elixir-lang/elixir/blob/main/lib/iex/lib/iex/config.ex), [`elixir.app.src` syntax colors](https://github.com/elixir-lang/elixir/blob/main/lib/elixir/src/elixir.app.src), [`elixir.erl` `ansi_enabled`](https://github.com/elixir-lang/elixir/blob/main/lib/elixir/src/elixir.erl), [`kernel/cli.ex`](https://github.com/elixir-lang/elixir/blob/main/lib/elixir/lib/kernel/cli.ex), [`IEx.configure` docs](https://github.com/elixir-lang/elixir/blob/main/lib/iex/lib/iex.ex).
- **Julia**: [`REPL.jl` prompts](https://github.com/JuliaLang/julia/blob/master/stdlib/REPL/src/REPL.jl#L918-L926), [`client.jl`](https://github.com/JuliaLang/julia/blob/master/base/client.jl#L8-L32), [`ConsoleLogger.jl`](https://github.com/JuliaLang/julia/blob/master/base/logging/ConsoleLogger.jl#L65-L191), [`util.jl` `text_colors`](https://github.com/JuliaLang/julia/blob/master/base/util.jl#L5-L34).
- **GHCi**: [`GHC/Utils/Ppr/Colour.hs`](https://github.com/ghc/ghc/blob/master/compiler/GHC/Utils/Ppr/Colour.hs#L66-L100).
- **Clojure `clj`**: [the `clj` script](https://github.com/clojure/brew-install/blob/1.12.3.1577/src/main/resources/clojure/install/clj), [`clojure/main.clj`](https://github.com/clojure/clojure/blob/master/src/clj/clojure/main.clj), [rlwrap's manual](https://github.com/hanslub42/rlwrap/blob/master/doc/rlwrap.man.in).
- **rebel-readline** at `d8573a6`: [`tools.clj` themes](https://github.com/bhauman/rebel-readline/blob/d8573a61aad5cbbd83532e2050b6595cfb5b13bb/rebel-readline/src/rebel_readline/tools.clj#L208-L242), [`clojure/main.clj`](https://github.com/bhauman/rebel-readline/blob/d8573a61aad5cbbd83532e2050b6595cfb5b13bb/rebel-readline/src/rebel_readline/clojure/main.clj#L113), [`line_reader.clj`](https://github.com/bhauman/rebel-readline/blob/d8573a61aad5cbbd83532e2050b6595cfb5b13bb/rebel-readline/src/rebel_readline/clojure/line_reader.clj#L748-L766), [`commands.clj`](https://github.com/bhauman/rebel-readline/blob/d8573a61aad5cbbd83532e2050b6595cfb5b13bb/rebel-readline/src/rebel_readline/commands.clj#L60-L75), [`jline_api.clj`](https://github.com/bhauman/rebel-readline/blob/d8573a61aad5cbbd83532e2050b6595cfb5b13bb/rebel-readline/src/rebel_readline/jline_api.clj#L36-L42).
- **REPLy** at `2b28587`: [`eval_modes/shared.clj`](https://github.com/trptcolin/reply/blob/2b28587004aa3b5cd5d4548eb87f7674eb999b13/src/reply/eval_modes/shared.clj), [`reader/simple_jline.clj`](https://github.com/trptcolin/reply/blob/2b28587004aa3b5cd5d4548eb87f7674eb999b13/src/reply/reader/simple_jline.clj), [Leiningen's `repl.clj`](https://github.com/technomancy/leiningen/blob/main/src/leiningen/repl.clj#L150-L168).
- **Racket CS**: [expeditor `color.rkt`](https://github.com/racket/expeditor/blob/master/expeditor-lib/private/color.rkt#L24-L55), [expeditor `main.rkt`](https://github.com/racket/expeditor/blob/master/expeditor-lib/main.rkt#L1530).
- **Scala 3**: [`JLineTerminal.scala`](https://github.com/scala/scala3/blob/main/repl/src/dotty/tools/repl/JLineTerminal.scala#L85-L89), [`SyntaxHighlighting.scala`](https://github.com/scala/scala3/blob/main/compiler/src/dotty/tools/dotc/printing/SyntaxHighlighting.scala#L26-L31), [`Rendering.scala`](https://github.com/scala/scala3/blob/main/repl/src/dotty/tools/repl/Rendering.scala#L265-L273), [`MessageRendering.scala`](https://github.com/scala/scala3/blob/main/compiler/src/dotty/tools/dotc/reporting/MessageRendering.scala#L64).
- **evcxr** at `360deef`: [`evcxr_repl/src/bin/evcxr.rs`](https://github.com/evcxr/evcxr/blob/360deef294d7261f4b9c79edb07b12fcbc489868/evcxr_repl/src/bin/evcxr.rs#L74-L197), [`evcxr_repl/src/repl.rs`](https://github.com/evcxr/evcxr/blob/360deef294d7261f4b9c79edb07b12fcbc489868/evcxr_repl/src/repl.rs#L71-L79), [`evcxr/src/errors.rs`](https://github.com/evcxr/evcxr/blob/360deef294d7261f4b9c79edb07b12fcbc489868/evcxr/src/errors.rs#L60-L100).

What the table shows:

- **No REPL colors the program's own output by default.** Only evcxr colors standard error, and REPLy does with `--color`.
- **The token colors agree.** Where a REPL colors values or input with 16 colors, strings are green (Python 3.14, Node.js, Deno, IEx) and numbers are yellow (Python 3.14, Node.js, Deno, IEx). Atoms and keywords, the nearest kin of `:x`, are cyan in IEx, and Node.js and Deno give cyan to keywords.
- **Comments are faint.** Node.js and Deno print them grey, and rebel-readline prints them in grey 243.
- **A prompt is plain or one color.** Node.js, Deno, `irb`, IEx, `clj`, rebel-readline and REPLy keep it plain. Python makes it bold magenta, Julia and IPython green, Scala magenta and evcxr yellow.
- **Most stay within the 16 base colors.** rebel-readline's default theme and evcxr's error labels are the exceptions, and IPython renders pygments hex colors.
- **The color switches differ.** Python, Node.js and `irb` honor `NO_COLOR` and `TERM=dumb` and check for a terminal. Python and Node.js add `FORCE_COLOR`. IPython, `irb`, Julia, GHCi, IEx and Scala give a flag. Most let the user change the theme.

## Options

The options below differ in what the prompt colors. Every option keeps today's diagnostic colors, so a session and a `check` run look the same.

### A: one decision for color, and nothing new colored

Fix the three faults of today and color nothing else. The CLI decides once, per stream, whether to color, and the diagnostics and clap's help follow that decision.

- It costs least, and a session stays as plain as Node.js or `clj`.
- The prompt, the values and the program's output still look alike, which is the problem this RFC must solve.

### B: color the session's own marks

Option A, plus colors on the lines the prompt writes itself:

| Element | Style | Escape | Why |
| --- | --- | --- | --- |
| prompt `> ` | bold bright blue | `1;94` | the diagnostics' gutter: the compiler's own marks share one color |
| continuation `  ` | none | | two spaces show no color |
| `note: the program exited with code N` | `note` bold bright green, text bold | `1;92`, `1` | rendered as a diagnostic note title through `annotate-snippets` |
| a definition | prints nothing | | as today |
| the program's standard output and standard error | plain, passed through | | as every REPL in the table |

The program keeps the terminal as its standard output and standard error, so a program that checks for a terminal behaves at the prompt as it does under `run`.

### C: option B, plus token colors from the reader

Option B, plus colors on source text and printed values, by the kinds of token that the reader and the expander know:

| Token | Style | Escape | Known by |
| --- | --- | --- | --- |
| string, quotes included | green | `32` | `FormKind::String` |
| number | yellow | `33` | `FormKind::Number` |
| keyword `:x` | cyan | `36` | `Keyword::field_name` in `expand/src/core/keyword.rs` |
| comment, `;` to the end of the line | dim | `2` | `Syntax::COMMENT` |
| `(`, `)` | plain | | `Syntax::OPEN`, `Syntax::CLOSE` |
| any other symbol | plain | | `FormKind::Symbol` |

The rule keys only on what the reader or the compiler knows. `true`, `false`, `none` and `(do)` read as a symbol and a list, so they print plain: a color for them would need a list of prelude names in Rust. No token is bold, so a value never takes the weight of a diagnostic's level, and the yellow of a number and the bold yellow of `warning` stay apart.

The colors apply in three places:

1. **Input as typed.** `rustyline`'s `Highlighter` colors the line as the user types it. The reader stops at an unclosed list, and a line under edit is often unclosed, so the highlighter needs the atoms and comments of partial text. A token scan in the reader module gives them, built from `Syntax` and `Syntax::ends_atom`, so the reader's rules stay stated once.
2. **A printed value.** RFC 0048 makes every printed value a form that the reader reads, on one line. The prompt reads the printed text back with the reader and colors each atom by its `FormKind`. The value is printed by the program, so the prompt must tell it apart from the program's output. It does so as `eval --json` does: the program prints a marker before the value, and the prompt reads the program's standard output through a pipe. The prompt MUST forward every byte that cannot begin the marker at once, and hold back only a tail that is a prefix of the marker, so a prompt the program prints before `(read-line)` shows at once. The program's standard output is then a pipe, not the terminal. The prompt writes the program's bytes to the raw standard output unchanged, whatever the decision, so a program's own escape codes reach the terminal as the program wrote them. Only the CLI's own styles follow the decision.
3. **A `doc` page.** The header of a page, such as `(defn twice (x))`, and the type line of `doc --types` are source, so they take the token colors. The docstring is Markdown and stays plain. The CLI receives the page as one string from the prelude's `doc`, and it MUST NOT split that string, because it would then depend on how the prelude builds the page. The prelude gives the header as its own form, `(header term)`, but no form for the rest of the page: `(docstring term)` is `L0327` on an undocumented term, and the blank line and `No documentation.` live inside `doc`. The build therefore adds `(page-body term)` to `prelude/macros.rslisp`, the text of the page after its header, blank line included, and states `doc` as the header followed by that text. The CLI expands `(header term)` and `(page-body term)`, colors the header with the token scan, and writes the body unchanged.

`eval` without `--json` pipes the program's standard output through the marker only when standard output decides to color, and then colors the value by the same rule. Otherwise the program keeps the stream and its value prints plain, as today. So with standard output piped and standard error a terminal, the program writes straight to the pipe, the value is plain, and the diagnostics are colored.

### D: option C, plus a user theme

Option C, with every style read from a variable, such as `RSLISP_COLORS="string=32:number=33"` in the shape of `GHC_COLORS`, or from a configuration file.

- IPython, Node.js, IEx, Julia, GHCi and rebel-readline let a user change the theme.
- The 16 base colors already follow the terminal's theme, so a user who wants other colors changes the terminal's palette. A variable adds a format to parse, to document and to report errors in, and the language has no configuration file yet (#294).

### When to color

Every option shares one decision. The CLI makes it once per stream at start-up, by `anstream::AutoStream::choice` in [`anstream` 1.0.0 `src/auto.rs`](https://github.com/rust-cli/anstyle/blob/anstream-v1.0.0/crates/anstream/src/auto.rs), so the diagnostics and clap's help share one rule. Its order is:

1. `--color always` or `--color never` decides, through `anstream`'s global `ColorChoice`.
2. A non-empty `NO_COLOR` turns color off ([no-color.org](https://no-color.org/)).
3. A non-empty `CLICOLOR_FORCE` turns color on.
4. `CLICOLOR=0` turns color off.
5. A stream that is a terminal is colored when `TERM` is set and is not `dumb`, when `CLICOLOR` is set and is not `0`, or when `CI` is set.
6. Otherwise the stream is plain.

So `TERM=dumb` turns color off unless `CLICOLOR` or `CI` turns it back on.

`eval --json` never colors: each `rendered` field and each `value` is plain, whatever the decision, because an editor parses them. Standard output and standard error decide apart, so `rslisp eval f '(g)' 2> log` colors the value and writes plain diagnostics to `log`.

**A `--color auto|always|never` flag.** cargo, rustc, GHC, Julia and IEx take one. It gives a user a switch that does not depend on the environment. clap renders help and usage errors while it parses, so a parsed value comes too late for them. `--color` is therefore a global argument: the CLI reads it from the arguments before clap parses them, up to a `--`, and sets `Command::color`, so help and usage errors follow it wherever it appears. `rslisp --color never --help` and `rslisp check --help --color never` print plain help, and a usage error under `--color never`, such as an unknown argument, prints plain. A value other than `auto`, `always` or `never` is itself a usage error, which clap reports under `auto`. Without it, `NO_COLOR` and `CLICOLOR_FORCE` cover both directions, at the cost of an environment variable on each call.

### Where each part is built

| Part | Pull request |
| --- | --- |
| one decision per stream, honored by the diagnostics and clap's help | its own small pull request, which merges before pull request 299 |
| the prompt's color and the exit note as a diagnostic note | pull request 299 |
| input highlighting, the reader's token scan, the prompt through `Highlighter::highlight_prompt` | the line-editing pull request (#291) |
| value colors in `repl` and `eval`, through the marker | after pull requests 297 and 298 land: 297 prints values as forms the reader reads, and 298 adds the marker |
| `doc` header and type line colors, and `(page-body term)` in the prelude | with the value colors, which share the token rule |
| `--color` | with the decision |

The workspace needs no new crate. `anstyle` 1.0.14 and `anstream` 1.0.0 are already in `Cargo.lock`, through `annotate-snippets` and `clap_builder`. The CLI adds `anstream` and `anstyle` as direct dependencies at the versions the lock holds, and states each style once as an `anstyle::Style` constant. `annotate_snippets::renderer` exposes its defaults as `DEFAULT_ERROR_STYLE`, `DEFAULT_NOTE_STYLE` and the rest, and re-exports `anstyle`'s types, so the prompt's style is `DEFAULT_LINE_NUM_STYLE`, read from the crate, and the palette cannot drift from the diagnostics. On Windows that constant is bold bright cyan, so the prompt is bright cyan there, as the gutter is.

## Recommendation

Option C, with the `--color` flag, built in the steps of the table above.

- It solves the problem: the prompt, a value, the program's output and a diagnostic each look distinct, as the sample session below shows.
- It adds one palette, not two. The prompt shares the gutter's bold bright blue, the exit note is a diagnostic note, and the token colors use regular weight where every diagnostic level is bold.
- It follows the table's consensus: strings green, numbers yellow, keywords cyan, comments faint, program output plain.
- Every token rule keys on the reader or on `Keyword`, so the compiler names no prelude term.
- It uses only the 16 base colors, bold and dim, so the user's terminal theme sets the real colors and both light and dark backgrounds work.
- It honors `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE` and `TERM=dumb` everywhere, by `anstream`'s rule, which today's diagnostics do not.

The sample session in the recommended scheme, which a prototype script outside the repository prints with real escape codes:

```text
> (defn square (n) (* n n))  ; a definition prints nothing
> (defstruct point :x i64 :y i64)
> (->point :x (square 2) :y 3)
(->point :x 4 :y 3)
> (pair "ana" (some 2.5))
(pair "ana" (some 2.5))
> (println "side " (square 3))
side 9
> (+ 1 "b")
error[L0401]: mismatched types
 --> <input 6>:1:6
  |
1 | (+ 1 "b")
  |      ^^^ expected an integer, found `str`
> (defn add-one (v) (push! v 1))
warning[L0504]: `add-one` changes an argument, so its name should end with `!`
 --> <input 7>:1:7
  |
1 | (defn add-one (v) (push! v 1))
  |       ^^^^^^^ this function changes an argument
  |
  = help: rename it `add-one!`
> (unwrap (the (option i64) none))
thread 'main' panicked at main.rs:9:10:
called `Option::unwrap()` on a `None` value
note: the program exited with code 101
```

## Decision

The project owner accepted option C on 2026-11-04, with these choices:

1. **The prompt** is bold bright blue, the color of the diagnostics' gutter.
2. **Value colors** put the program's standard output through a pipe at the prompt, as option C describes. The owner accepted that the program's standard output is then not the terminal. This also covers `eval` without `--json`: it pipes the program's standard output through the marker only when standard output decides to color. Otherwise the program keeps the stream and its value prints plain.
3. **Parentheses** stay plain.
4. **A definition** prints nothing.
5. **The `--color auto|always|never` flag** is added.
6. **The color decision** for the diagnostics and clap's help is its own small pull request, which merges before pull request 299.
7. **Docstrings** stay plain. Only a page's header and the type line of `doc --types` take the token colors.

## Rejected options and why

- **A:** it leaves the session as hard to scan as today.
- **B alone:** it marks the prompt but leaves each value as plain as the program's output, and the line-editing pull request needs the token colors anyway.
- **D:** the 16 base colors already follow the terminal's theme, and a theme format adds a parser, its errors and its documentation for little gain.
- **256 colors or true color:** a fixed shade looks wrong on one of a light or a dark background, as rebel-readline's two 256-color themes and its detection of the background show.
- **Coloring the program's standard error:** it needs a pipe, so the program loses its terminal, and the program's text is not the compiler's to style. Only evcxr does it by default.
- **Colors keyed on prelude names, such as `true` or `defn`:** the compiler would restate the prelude in Rust, which `AGENTS.md` forbids.

## Rule

[docs/language.md: Evaluation](../docs/language.md#evaluation). The build that lands the decision states there when the CLI colors and the color of each element. [docs/diagnostics.md](../docs/diagnostics.md) gains a line that diagnostics follow the same decision.
