# Writing diagnostics

This guide covers every error the compiler prints. A diagnostic uses rustc's layout and the tone of Elm's error messages.

## Layout

**Build every diagnostic from these parts, in this order:**

| Part | Content | Required |
|---|---|---|
| Header | `error[L0001]: title` | yes |
| Location | `--> path:line:column` | yes |
| Primary label | A caret under the exact form at fault, with a short message | yes |
| Secondary labels | Other forms that explain the error, such as the earlier definition of a duplicate name | no |
| Notes | `note:` lines that state facts the reader needs | no |
| Help | `help:` lines that state a fix | yes, if a fix exists |

```text
error[L0201]: cannot find `pointx` in this scope
 --> src/main.rslisp:4:10
  |
4 |   (print (pointx origin))
  |           ^^^^^^ not found in this scope
  |
help: a function with a similar name exists
  |
4 |   (print (point-x origin))
  |           ~~~~~~~
```

## Tone

**Write the title as a plain statement of what is wrong.** Write "cannot find `pointx` in this scope", not "unresolved symbol".

**Point the primary label at the smallest form that is wrong.** A label on a whole function hides the fault.

**If a fix exists, give it in `help:`.** If the fix is an edit, show the replacement text.

**Use the names the user wrote.** Never show a Rust name such as `make_point` in place of `make-point`.

**If a name does not resolve, suggest the closest name.** Rank candidates by edit distance. If no candidate is within a third of the name's length, leave out the suggestion.

**If the error is inside macro output, name the expansion.** Add `note: in this expansion of` followed by the macro name, with a label on the user's macro call.

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

## Tests

**Add a fixture and a snapshot for every new diagnostic.** Place the fixture under `tests/fixtures/`, in the directory of the stage that reports it. Use fake data only.
