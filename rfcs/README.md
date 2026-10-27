# rslisp RFCs

**An RFC records one design.** The RFC holds the problem, the options, the decision and the reasons. `docs/language.md` states the current rule, and the RFC links to it. **NEVER let an RFC be the only statement of a current rule.**

## Names

- **Name each RFC `rfcs/NNNN-short-name.md`.** Use four digits and a short name in lower case with hyphens between words.
- **Number each RFC with four digits.** `D1` to `D37` take the numbers up to `0037`. Give a new RFC the number after the highest RFC file in this folder.

## Lifecycle

- **A design pilot writes the RFC with status `Proposed` as its deliverable.** The RFC holds the options, the recommendation and the prototype.
- **The project owner decides.** The RFC records the decision and becomes `Accepted` or `Rejected`.
- **Merge the RFC with the build or alone.** Do not wait for the build to record a decision.

## Supersession

- **A later RFC that changes a rule marks the earlier one `Superseded by NNNN`.** The later RFC states the new rule.
- **NEVER edit an accepted RFC's reasoning after it merges.** Change only its status line.

## Decisions before RFCs

`rfcs/0000-decisions-before-rfcs.md` holds the decisions made before RFCs existed. It records `D1` to `D37` for lookup. `docs/language.md` remains the only statement of each current rule.

## Template

Copy `rfcs/template.md` for a new RFC. Fill every section. Leave no section empty.
