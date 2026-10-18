# Proposal

## Why

The first soak day (2026-10-17 into 10-18, two projects, 369 findings) put misfires at roughly a third of everything the hook said, and only ONE got disputed: agents comply instead of arguing, warnings included, and a warn misfire already cost a test its case labels. A good share of the misfires come from the binary rather than any rule, so no ruleset edit can tune them away (the private ruleset took its own fixes the same day).

## What Changes

- **Change detection ignores reflow and moved text.** A line counts as changed only when a word on it changed, so re-wrapping a paragraph changes nothing. Text that already exists at git HEAD in the same repo counts as unchanged wherever it lands, so splitting a file doesn't re-litigate its old comments (the soak's split of a server's bootstrap into its own module drew 28 blocks on comments nobody wrote that day).
- **Judge findings must quote changed words.** Today a one-word edit sends the whole paragraph as CHANGED and the judge can quote any sentence in it: 29 of the 74 `repeats` quotes were text the edit didn't write.
- **Judges say whether it's a violation.** The verdict schema gains a required `violates` flag and a finding with `violates: false` is dropped (one judge quoted text to explain it was out of scope, and the finding was kept).
- **Trailing comments see their own line.** A comment after code on the same line gets that code first in its context, so `# lwz` gets checked against the opcode it names instead of the line after it.
- **Sentence counting stops overcounting.** An ellipsis never ends a sentence, neither does `?` or `!` right before a closing quote (`"Trouble connecting?" after 90s`), sentences inside list items don't count toward a limit and a divider comment (`--- Section ---`) is a label rather than prose, splitting the block it sits in.
- **File links stop calling ambiguous files missing.** A name that matches several files gets a warning saying so (today it says "not found") and a mention of a directory is left alone.
- **Oxford comma findings quote the list.** Today every one matches `,`, so two in one file share a dispute id and the second sample overwrites the first (found writing this proposal: the dispute meant for one landed on the other).
- **Per-rule and per-judge `exclude`, per-judge `model` and `effort`.** A link rule can skip OpenSpec change folders (their relative links break when `/opsx:archive` moves the folder one level deeper) without the folder losing every other rule, and one judge can run at a higher effort without paying for it on all of them.
- **The dispute hint says what to do with a wrong warning:** dispute it and keep the text. Disputes stay record-only (the next run flags the same text).
- The Oxford comma corpus gains the soak's `X, aside, and Y` shape as a documented known misfire (see Non-goals).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `hook-intake`: what counts as a changed line
- `deterministic-rules`: per-rule `exclude`, sentence boundaries, sentence limits on lists, ambiguous and directory mentions, what an Oxford comma finding matches
- `prose-extraction`: divider comments
- `llm-judges`: per-judge settings, trailing-comment context, quotes on changed words, the `violates` flag
- `disputes`: the hint's wording, which span a sample saves

`hook-intake`, `deterministic-rules` and `prose-extraction` still live in aa's change folder and `disputes` in ac's, so ad archives AFTER both of them (its deltas need their requirements in `openspec/specs/` first).

## Non-goals

- A fix for the Oxford "aside" shape. Every heuristic tried also killed real lists, so the corpus pins the misfire and the case flips the day a signal turns up (design.md has what was tried).
- Catching edits made through Bash (`sed -i`, a python one-liner). The hook only sees Write / Edit / MultiEdit, and the backlog's Bash interception work is where that belongs.
- Word-level scoping for deterministic rules. Touching a line still means fixing the tell on it (aa's call): a line you rewrote is yours, and only reflow stops counting as a rewrite.
- The private ruleset. It lives outside this repo and took its fixes directly.

## Impact

- [docs/design.md](/docs/design.md) owns every subsystem this touches (hook contract, prose extraction, rule kinds, judges, disputes, the ruleset file). [README.md](/README.md) doesn't change: the new ruleset keys belong to the design doc's config reference. (Both links are root-relative so they survive the archive move.)
- Reach: CI on every push; users pick it up on their next `cargo install`.
- One-way door: the new ruleset keys (`exclude` on rules and judges, `model` and `effort` on judges). Old rulesets load unchanged, but one that uses them fails strict validation on an older binary, which shows up as the broken-ruleset note on every write until the binary catches up.
- The judge harness version bumps for `violates`, so the verdict cache misses once on every input.
- Moved-text detection runs `git` only when a write has findings to report, inside a git repo; no git, or git failing, keeps today's behavior. A task remeasures the design doc's performance numbers with it in place.
- No new dependencies (`similar` already does word diffs).
