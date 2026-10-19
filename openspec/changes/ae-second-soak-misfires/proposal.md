# Proposal

## Why

ad's binary went in on 2026-10-18 and the soak ran on it overnight. ad's hint worked: 51 disputes came in by 10-19 07:34Z, triage upheld 50 of them and not one agent rewrote text to satisfy a wrong warning. That makes disputes the soak's data now instead of misfires inferred from samples. 33 of the upheld ones were ruleset-side and the private ruleset took those fixes the same morning. The other 17 come from the binary, where no ruleset edit can reach them: this change fixes 15 and pins the last 2 as known misfires.

## What Changes

- **Judge quotes are matched by position, not by word.** ad's change test kept a quote when the change on its line shared ANY word with it, or when that line had a deletion anywhere. Plenty of docs write a paragraph as one line, so an edit that inserts "the" mid-paragraph lets a judge quote any old sentence in it, and so does one that deletes a word: 3 `repeats` disputes were sentences nobody touched, one labelled "Pre-existing text" by the agent that filed it. ad's own requirement already says that finding gets dropped ("Old sentence in a changed paragraph"); the implementation only held for paragraphs wrapped across lines.
- **A missing file mention warns only when it looks like a stale reference.** All 11 link disputes were "not found" on files that can't be in a repo: a manifest written at runtime and paths inside a disc image the user supplies. A path that doesn't resolve will warn only when its parent directory exists, so `src/gone.rs` next to a real `src/` still warns. A bare backticked name that doesn't resolve goes quiet, like an unmarked name does today. A broken explicit link still warns.
- **An Oxford comma item starting with "not" is an aside, not a list item.** That shape ("reads the saved copy, not the live one, and the cache...") was 1 of the 3 aside disputes. The other two set off a value ("sets the timeout, 5 s, and retries") and join the corpus as known misfires.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `llm-judges`: Findings only on changed lines (a quote's own words have to hold the change)
- `deterministic-rules`: File link findings (which unresolved mentions warn), Oxford comma rule (the "not" aside)

Both requirements were last changed by ad, and `deterministic-rules` still lives in aa's change folder, so ae archives AFTER ad (which waits on aa and ac).

## Non-goals

- The value aside. A value set off by commas looks like a list item to every count-based test, so the corpus pins it next to ad's `X, aside, and Y` cases.
- Positions for the deterministic rules. They're scoped by line (aa's call: a line you rewrote is yours) and that doesn't change.
- The 33 ruleset-side disputes: OpenSpec spec files skipped by `repeats` and `verdict-first`, plus narrower rubrics for inventories, rejected options and field docs. They're in the private ruleset.

## Impact

- [docs/design.md](/docs/design.md) owns all three: the Judges section's "Same decision" bullet (the change test for quotes), `file_links` and `oxford_comma`. [README.md](/README.md) doesn't change.
- Reach: CI on every push; users pick it up on their next `cargo install`. No ruleset format change, so no one-way door.
- A whole-scope judge gets fewer findings on one-line paragraphs, since its quotes were only ever checked by this test (the check against HEAD skips whole judges).
- Docs that name files outside the repo get fewer warnings, and a typo in a bare backticked name (`READNE.md`) no longer gets caught. Linking it still catches the typo, since a broken link warns.
- The change set keeps the new text so it can place a quote, which costs one copy of the file per write.
- [openspec/backlog.md](/openspec/backlog.md) loses its `ae` and `af` labels, since ids get allocated when a change is proposed.
