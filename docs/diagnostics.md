# Writing diagnostics

This guide covers every error the compiler prints. A diagnostic uses rustc's layout and the tone of Elm's error messages.

## Layout

**Build every diagnostic from these parts, in this order:**

| Part | Content | Required |
|---|---|---|
| Header | `error[L0001]: title`, or `warning[L0001]: title` for a warning | yes |
| Location | `--> path:line:column` | yes |
| Primary label | A caret under the exact form at fault, with a short message | yes |
| Secondary labels | Other forms that explain the error, such as the earlier definition of a duplicate name | no |
| Notes | `note:` lines that state facts the reader needs | no |
| Help | `help:` lines that state a fix | yes, if a fix exists |

A warning does not stop the build: the compiler prints it and continues.

**The CLI colors diagnostics only when standard error's color decision is to color.** [docs/language.md: Colors](language.md#colors) states the decision.

**The CLI prints at most `CappedReport::MAXIMUM_PRINTED_DIAGNOSTICS` diagnostics, in source order, then one `note:` line that counts the rest.** Every stage still returns every diagnostic.

```text
error[L0201]: cannot find `distnce` in this scope
 --> src/main.rslisp:4:10
  |
4 |   (print (distnce origin target))
  |           ^^^^^^^ not found in this scope
  |
help: a function with a similar name exists
  |
4 |   (print (distance origin target))
  |           ~~~~~~~~
```

## Tone

**Write the title as a plain statement of what is wrong.** Write "cannot find `distnce` in this scope", not "unresolved symbol".

**Point the primary label at the smallest form that is wrong.** A label on a whole function hides the fault.

**If a fix exists, give it in `help:`.** If the fix is an edit, show the replacement text.

**Use the names the user wrote.** Never show a Rust name such as `make_point` in place of `make-point`.

**If a name does not resolve, suggest the closest name.** Rank candidates by edit distance. If no candidate is within a third of the name's length, leave out the suggestion.

**If the error is inside macro output, name the expansion.** Add `note: in this expansion of` followed by the macro name, with a label on the user's macro call. When a macro expands to an `if`, a note about the `if`'s test or branches names the user's macro instead of `if`.

**Never blame the reader.** Describe the code, not the person.

## Codes

**Give each diagnostic its own code with the `L` prefix.** The first digit groups codes by stage:

| Range | Stage |
|---|---|
| `L01xx` | Reader |
| `L02xx` | Names, modules and hygiene |
| `L03xx` | Macros |
| `L04xx` | Types |
| `L05xx` | Ownership |
| `L06xx` | Emitting and building |

**Keep rustc's own code on an error that rustc reports.** Add `note: reported by rustc` and rewrite every Rust name in the message to the name the user wrote.

**Name the definer the user wrote.** A definition inside a body is `L0208`, and its title names the head of the outermost macro call that the user wrote around the `def` or `struct`: `(defn inner () 1)` in a body reports `` `defn` can only appear at module level ``, a `deftest` reports `deftest`, and a user macro over `defn` reports the user macro. A `def` or `struct` that the user wrote keeps its own name.

**Name a repeated test at the second `deftest`.** A second test of one name in one module is `L0333`, ``test `pads` is defined more than once``, with its primary label on the second test's name and a secondary label on the first, and the help `rename one of the tests`.

**Report each error once.** A macro can copy one form of its input into several places of its output, so the expander drops a recovered error, or an error of a form that a top-level `do` splices, that repeats an earlier error of the same top-level form with the same code, title, labels and notes. Of the copies, it keeps the one with the most helps. The type stage does the same for the diagnostics of a whole program: a macro's output can give two forms one location, so two errors can match exactly.

**Report a missing annotation only when no other error explains it.** A `type annotations needed` error is a symptom of another error, so the type stage drops it when the program has any other error.

**Report an unresolved type variable once.** A value that holds the variable reports it first, and a trait obligation reports only a variable that no value holds. Variables that inference merges later count as one.

## Tests

**Add a fixture and a snapshot for every new diagnostic.** Place the fixture under `tests/fixtures/`, in the directory of the stage that reports it. Use fake data only.
