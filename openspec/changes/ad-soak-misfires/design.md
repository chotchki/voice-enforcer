# Design

## Context

Motivation is in [proposal.md](proposal.md) (Why). The evidence is the 2026-10-18 triage of 369 saved samples (181 judge, 188 deterministic) plus what the hook said about this change's own artifacts while they were written. What exists today:

- `changed_lines` in [src/hook/mod.rs](/src/hook/mod.rs) runs `similar::TextDiff::from_lines` against the file on disk; a new file is all changed. Both rules and judges use that set.
- Judges get whole spans marked CHANGED, and `verify::locate` keeps a quote found anywhere in a changed span.
- `Sample::new` in [src/disputes.rs](/src/disputes.rs) re-derives a finding's span from its line, and samples take `[judges]` model and effort for every judge.
- Comment pieces already know whether they're `own_line` ([src/extract/code.rs](/src/extract/code.rs)); `code_context` starts at the row after the comment.
- The Oxford rule's finding matches `,`, and ids hash `(session, path, rule, matched)`, so every Oxford finding in a file shares one id. ac's design called id collisions vanishingly unlikely; it was reasoning about the hash, and the collision comes from identical inputs.

## Goals / Non-Goals

**Goals:**

- A finding means Claude (or this write) put that text there. Reflow, re-indentation and moving existing text stop counting as writing it.
- Every misfire class in the triage that has a deterministic cause gets a fix and a synthetic fixture.
- A ruleset owner can scope a rule or a judge to where it works, and tune one judge's cost without touching the rest.

**Non-Goals:**

- Better judge rubrics. Those live in private rulesets (the owner's lever), and the replay numbers for them stay out of this repo.
- Marking changed words inside the judge input (see Risks: revisit if the next soak shows crowded-out findings).

## Decisions

### Change is word-level, deletions included

Today's line diff still finds the hunks (runs of non-equal ops). Inside each hunk a second diff runs over the whitespace-separated tokens alone: every inserted or replaced token marks its line changed and records its words (lowercased letter / digit / `_` runs, so `**Bold**` and `[bold](x.md)` both hold `bold`), and a deletion marks the line it happened on. Whitespace never enters that diff (kept in, a space can out-match a word for the LCS, which the first cut of this did), so a re-wrapped paragraph and a re-indented block change nothing. Whole deleted lines sit in a hunk with no new lines, so they still change nothing, same as today.

Alternatives: a line diff over whitespace-normalized lines can't see reflow (the line boundaries themselves move); comparing normalized paragraphs only works for markdown. The token diff runs with a 100ms deadline, past which `similar` approximates, and an approximate diff only ever marks MORE as changed (today's behavior at worst, never a hidden change).

### Judges must quote changed words, rules keep line granularity

A judge finding survives only if its quote holds a word the write changed on the quote's lines, or one of those lines had a change with no word in it (a deletion, a bare dash). Spans map raw text to lines, not source bytes (pulldown-cmark's text events don't map byte-for-byte anyway), so the match is by word per line: a changed word that also appears in an untouched sentence on the same line lets that sentence through, which is today's behavior for that line. Deterministic findings keep the line test on purpose: their matches are short and line-bound, and "you touched the line, fix its tell" is aa's call and still the right one. Judges read whole paragraphs, which is why the line test was too coarse for them (29 of 74 `repeats` quotes predated the edit).

### Text already at HEAD is unchanged

After scoping, and only when findings remain, the hook finds the repo root (walking up to `.git`, file or directory, so worktrees count) and checks each span holding a finding: its source lines, whitespace collapsed, searched in HEAD. One `git grep -F -l` over HEAD, with one `-e` needle per span (the span's longest trimmed line, skipped under 8 characters), finds candidate files, then one `git cat-file --batch` reads them (at most 200) for the whole-text compare. Results are memoized per span, so the deterministic pass and the judge pass share them. Findings in a span found at HEAD are dropped, for rules and diff judges. A whole judge keeps them, since a paragraph copied from elsewhere is exactly what `repeats` is for.

HEAD rather than the working tree, because the working tree holds text Claude wrote earlier in the same session; counting that as "existing" would launder it. Git's own copy detection (`git diff -C`) can't do this job: Claude usually writes the new file BEFORE editing the old one, so there's no deletion side to pair with yet.

Bounded: at most 50 spans and a 2s budget per write, after which the remaining findings are reported as they are today. Any git failure means the same. The cost only lands on writes that have findings, so a clean write pays nothing.

### `violates` is in the schema, not the prompt

The verdict schema gains a required boolean, `HARNESS_VERSION` goes to 3 (one cache miss per input) and `violates: false` findings get dropped in `verify`. A harness line ("never report text to clear it") would be a prompt Haiku can drift from; the flag makes it commit per finding for about one token. 1 of 181 findings had this shape, so this is cheap insurance rather than a big win.

### Findings carry their span

`Finding` gains the index of the span it came from (rules have it at creation, judges get it from `locate`), and `Sample::new` uses it instead of matching lines. That fixes table rows, where every cell sits on one line. Samples also record the model and effort the judge actually ran with, now that a judge can override them.

### Trailing comments get their own row

When a comment piece isn't `own_line`, its code context opens with that row's text up to the comment, then the usual 20 lines after. The judge reads `elif op == 32:` with `# load word` instead of the line after it.

### Counting sentences

- An ellipsis (a terminal run of two or more `.`, or one holding `…`) never ends a sentence.
- `?` or `!` directly before a closing quote never ends one. That undercounts `He asked "why?" Then left.`, the safe direction for a limit (a missed warning beats a false block).
- Capitalization stays out of it, per aa.
- `sentence_limit` counts only text outside list items. A list item starts at a line opening with `-`, `*`, `+` or `N.` / `N)` and runs to a blank line or the next item, so prose that follows a list with no blank line in between gets swallowed by it (an honest gap; a comment written that way is rare).
- Divider comments (a non-doc line comment opening with three or more `-`, `=`, `*` or `~`) are dropped at extraction and break the `own_line` merge. `#` is out of the set because `### Errors` is a rustdoc heading, and doc comments are out because splitting a `///` block would knock its top half down to line level.

### Oxford findings quote the list

The finding's matched text runs from the first item (after the clause split) through the conjunction, trimmed and capped at 100 characters like judge quotes. Ids hash it, so they stop colliding.

The `X, aside, and Y` shape (a comma-bracketed aside before a clause: "saves the file, open or not, and closes the tab") stays a known misfire, pinned in the corpus as an expected hit with a comment saying so. What was tried and why it lost:

- Capping the first item's words too: kills lists whose first item carries the subject ("This release adds Rust, Swift, and Python"), and masked inline code collapses to one token, so an aside after `` `x` `` slips through anyway.
- Requiring the final conjunct not be a clause: needs part-of-speech tagging ("and nothing reads them" vs "and gamma"), which a deterministic rule doesn't have.
- Requiring three items before the conjunction: misses every plain three-item list, the common case.

### Ambiguous names and directories

Resolution gains two outcomes. `Ambiguous(n)` is a `warn` saying how many files match; a bare maybe-name stays silent, same as when it doesn't resolve. `Directory` (the mention exists as a directory under the markdown file's directory or the repo root) is silent rather than a link demand, because plenty of mentioned directories are git-ignored tool installs, and a link to one is broken on GitHub.

### Per-rule and per-judge settings

`exclude` on a rule or judge builds its own `GlobSet` with the same `~` expansion and absolute-path matching as the top-level list; `rules::evaluate` and the judge scheduler skip the rule or judge for a matching path. The `check` directory walk still uses only the top-level excludes (a file one rule skips is still a file). `model` and `effort` on a judge override `[judges]` for its calls; the cache key already hashes both, so no cache change.

Root-relative links (`/docs/design.md`) were the other answer to OpenSpec folders moving on archive, and they already pass (the rule ignores absolute targets), so these artifacts use them. They aren't the default suggestion because they only work on GitHub-style renderers.

### The hint

```text
Think a finding is wrong? Run `/abs/voice-enforcer dispute <id> --reason "why"`. It records the finding for the ruleset owner; it doesn't change this result. A wrong warning needs no rewrite: dispute it and keep the text.
```

The last sentence appears only when the listing holds a warning. Blocks keep today's text, since the record-only design means a disputed block still has to be rewritten (or hit the loop guard).

## Risks / Trade-offs

- [A tell copied from elsewhere in the repo passes the deterministic rules] → Accepted: it was already in the repo, which is the line aa draws for legacy text. `repeats` still sees the copy.
- [Judges keep quoting unchanged sentences, those findings get dropped and a real violation in the changed sentence goes unreported] → Watch the next soak's samples for diff judges going quiet on edits; the fix then is marking changed words in the input (a harness version bump).
- [`git grep` over a large repo is slow] → Only runs with findings in hand, at most 50 spans, 2s budget, after which findings are reported as today.
- [The word diff is slow on a huge file] → `similar`'s deadline plus the line-diff fallback.
- [List-item exclusion lets a long bulleted ramble through `sentence_limit`] → It's a length proxy either way; the profile's quarrel is with fluff, which the judges cover.
- [A per-rule exclude hides a rule from a whole folder] → The owner's call, and `validate` keeps reporting the rule as loaded.

## Migration Plan

Nothing to migrate: every new key is optional, so existing rulesets load as they are. The judge cache misses once (harness 3). Oxford ids change shape, so samples and disputes recorded before this land keep the old `,` match (they're records, nothing reads them back). Rollback is the previous binary, and a ruleset that started using the new keys then needs them removed (the strict-validation one-way door from the proposal).

## Open Questions

- Should `repeats` run at `effort = "high"` once the override exists? It's a ruleset question, so it gets answered by replaying the soak's `repeats` samples at both efforts after this lands.
