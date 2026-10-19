# Design

## Context

Motivation is in [proposal.md](proposal.md) (Why). The evidence is 49 disputes filed between 2026-10-18 22:09Z and 10-19 07:34Z on ad's binary, plus one more the hook produced on this change's own proposal. What exists today:

- [src/changes.rs](/src/changes.rs) records, per changed line, the set of words the write added or replaced and an `opaque` flag for a line with a deletion or a word-less change. `quote_overlaps` keeps a quote when a line in its range is opaque or shares any word with it.
- `verify::locate` in [src/judges/verify.rs](/src/judges/verify.rs) finds the quote in the span's raw text and maps it to source LINES. Spans keep `(raw offset, line)` segments and no source bytes.
- The verdict cache sits before `verify`, so a cached verdict gets filtered by whatever change test the binary has.
- `file_links` warns "not found" for a `Path` mention (holds a `/`) and a `KnownName` (backticked, known extension) that don't resolve, and stays quiet for a `MaybeName`.
- `oxford::is_list` takes the comma-separated parts before the conjunction and rejects only empty parts, long parts and a lone intro word.

The live case: this change's proposal had "every one checked and upheld, and" rewritten to "triage upheld every one and" on a one-line paragraph. The rewrite deleted words, the line went opaque and problem-without-solution's quote of a different sentence on that line got through.

## Goals / Non-Goals

**Goals:**

- A judge quote passes only on text the write changed, wherever the paragraph's line breaks fall.
- "Not found" means a likely stale reference, not "a file this repo doesn't hold".
- Every fix gets a synthetic fixture, and the misfires still standing get pinned in the corpus.

**Non-Goals:**

- Source byte offsets in spans. Extraction stays as it is (see the first decision).
- Any change to how rules are scoped. They keep the line test.

## Decisions

### Place the quote by its words, on the source lines

The change set keeps the new text, and per changed line the byte range of each added or replaced token and the offset of each deletion (the next new token's start, or the hunk's end). To test a quote, its word keys (the same lowercased letter / digit / `_` runs used today) are aligned in order against the word keys of the source lines `locate` reported: each alignment starts at an occurrence of the quote's first word and matches the rest in order, skipping at most 16 source words between two matches. Those skips are what link targets, inline HTML and entities add to the source. The alignment's window runs from the start of its first matched word's whitespace token to the end of its last one's, so the quote's trailing punctuation is inside. The quote overlaps when a changed token intersects the window or a deletion offset falls inside it, ends included, for ANY alignment. With every alignment checked, a phrase that appears twice on a line can't hide a change in the second copy.

A quote that doesn't align at all (an entity that decodes to letters, `caf&eacute;` against "café") falls back to today's test on those lines, so the error stays on the side of reporting too much.

Alternative: carry source byte offsets through extraction (pulldown-cmark's offset iterator for markdown, tree-sitter node ranges for comments) and map the quote exactly. More exact, but it changes every extractor and the segment structure every rule reads, to fix one consumer. The alignment stays inside the change set and `verify`.

Alternative: drop stop words from the set test. That fixes the "the" case and not the deletion one, which is the commoner leak on one-line paragraphs (every rewrite that shortens a sentence deletes something).

### A missing path warns only when its parent directory exists

For a `Path` mention that resolves to nothing, the rule checks whether its parent directory exists beside the markdown file or at the repo root. If it does, the reference is probably stale (`src/gone.rs` with a real `src/`) and it warns as today. If it doesn't, the path names something outside the repo (a disc image's tree, a runtime directory, another machine) and it's silent. A `KnownName` that doesn't resolve goes silent like a `MaybeName`, but still warns when it's ambiguous, since "matches 3 files" is useful either way.

Alternative: look for the parent anywhere in the repo index. Every crate in a workspace has a `src/`, and generic names (`level/`, `assets/`) would match by accident and bring the misfire back.

Alternative: drop "not found" for mentions entirely. That loses the stale-path case, the one the warning exists for.

Alternative: a per-rule `missing = "off"` key. That's a ruleset format change (a one-way door for older binaries) to switch off something nobody's disputes defend.

### A part starting with "not" ends the list

`is_list` rejects the list when any part after the first starts with the word "not". "X, not Y, and Z" is a contrast aside in every soak case, and a real list doesn't put "not" at the head of an item. The two value asides ("sets the timeout, 5 s, and retries") stay misfires: a number or a measurement is a fine list item ("sizes 8, 16 and 32"), so neither the part's shape nor its word count tells them apart. They go into `aside_before_a_clause_still_fires` as synthetic cases.

## Risks / Trade-offs

- [The alignment's 16-word skip lets a short quote match across unrelated words on a long line] → only when the quote's words appear in order, and an over-match only widens the window (more reported, never fewer).
- [A typo in a bare backticked name stops warning] → a link still warns when broken, and a path still warns when its directory exists. The soak had 11 disputes on this warning and no case where it caught a real typo.
- [Keeping the new text in the change set] → one copy of the file per write, dropped with the change set. No latency task: the alignment runs only for judge findings, over a few lines each.
- [ae archives behind ad, which waits on aa and ac] → nothing breaks while it waits, the deltas just stay in the change folders. Closing AA.19b (the soak day) once this triage lands lets aa archive, and ac, ad and ae follow in that order.
