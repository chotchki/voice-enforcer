# Spec Delta

## ADDED Requirements

### Requirement: Per-rule excludes

A rule MAY set `exclude` to a list of globs, matched against absolute paths the same way as the ruleset's top-level `exclude` (a leading `~` expands to the home directory). The rule SHALL NOT run on a file matching one of its globs, and every other rule SHALL still run on that file. An invalid glob SHALL be a load error naming the rule and its line.

#### Scenario: One rule skips a folder
- **WHEN** a `file_links` rule sets `exclude = ["**/openspec/changes/**"]` and a phrase rule sets nothing, and a file under `openspec/changes/` names an unlinked file and holds the banned phrase
- **THEN** only the phrase finding is reported

#### Scenario: Bad glob
- **WHEN** a rule's `exclude` holds `"[unclosed"`
- **THEN** loading fails and the error names the rule

## MODIFIED Requirements

### Requirement: Oxford comma rule

An `oxford_comma` rule SHALL report a finding for a comma placed directly before the final `and` or `or` of a list, where a list is at least two earlier comma-separated items in the same sentence, each at most `max_item_words` words long (default 4). A comma before `and` or `or` with no list before it in the sentence SHALL NOT be a finding. The finding's matched text SHALL be the list from its first item through the conjunction, so two findings in one file never match the same text unless they quote the same list.

#### Scenario: Serial comma flagged
- **WHEN** the prose reads "red, green, and blue"
- **THEN** one finding is reported on the comma before "and", matching "red, green, and"

#### Scenario: No serial comma
- **WHEN** the prose reads "red, green and blue"
- **THEN** no finding is reported

#### Scenario: Two independent clauses
- **WHEN** the prose reads "I fixed the bug, and the build passes now."
- **THEN** no finding is reported

#### Scenario: Two lists in one file
- **WHEN** a file holds "Pick tea, coffee, or water." and later "Bring forks, plates, and cups."
- **THEN** the two findings have different matched text and different dispute ids

### Requirement: Comment sentence limits

A `sentence_limit` rule SHALL apply only to the comment register and SHALL hold a `max_sentences` limit for any of the `line`, `item` and `module` levels. A comment block with more sentences than the limit for its level SHALL produce one finding stating the count and the limit, on the block's first line. Sentences inside a list item SHALL NOT count toward the limit, where a list item starts at a line opening with `-`, `*`, `+` or a number followed by `.` or `)` and runs until a blank line or the next item.

#### Scenario: Doc comment over its limit
- **WHEN** the `item` limit is 2 and a `///` block above a function holds three sentences
- **THEN** one finding reports 3 sentences against a limit of 2

#### Scenario: Level without a limit
- **WHEN** the rule sets limits only for `line` and `item`
- **THEN** module-level comments are never reported by it

#### Scenario: Bulleted module doc
- **WHEN** the `module` limit is 5 and a `//!` block holds three sentences of prose followed by six bullets of two sentences each
- **THEN** no finding is reported

### Requirement: Sentence boundaries

A sentence SHALL end at `.`, `!` or `?` followed by whitespace or the end of the block, with capitalization never a signal. Common abbreviations (`e.g.`, `i.e.`, `etc.`, `vs.`), decimals, version numbers, file names, masked inline code, a number opening a line (`1.`), an ellipsis (two or more `.` in a row, or `…`) and a `?` or `!` directly before a closing quote SHALL NOT end a sentence. A block with no terminal punctuation SHALL count as one sentence.

#### Scenario: Abbreviations and versions don't split
- **WHEN** a line comment reads "Pins v1.2.3 of the client, e.g. for retries."
- **THEN** it counts as one sentence

#### Scenario: Lowercase starts still split
- **WHEN** a comment reads "first thing. second thing."
- **THEN** it counts as two sentences

#### Scenario: Ellipsis in a usage line
- **WHEN** a line comment reads "run <file> [args ...]  (flags go last)"
- **THEN** it counts as one sentence

#### Scenario: Quoted question
- **WHEN** a line comment reads "Shows \"Retry?\" after 5s, then gives up."
- **THEN** it counts as one sentence

### Requirement: File link findings

A mention that resolves SHALL be a finding at the rule's severity, with a suggestion holding the relative link from the markdown file to the resolved file. A path or inline-code mention that does not resolve SHALL be a `warn` finding saying the named file wasn't found, and one that matches more than one file SHALL be a `warn` finding saying how many files it matches, whatever the rule's severity. A mention that names an existing directory SHALL produce no finding. A relative link whose target does not exist SHALL be a `warn` finding.

#### Scenario: Unlinked existing file
- **WHEN** `docs/guide.md` mentions `src/main.rs` in backticks, not as a link, and `src/main.rs` exists at the repository root
- **THEN** a finding at the rule's severity suggests `[src/main.rs](../src/main.rs)`

#### Scenario: Linked file is fine
- **WHEN** the prose holds `[src/main.rs](src/main.rs)` and the file exists
- **THEN** no finding is reported

#### Scenario: Named file is missing
- **WHEN** the prose mentions `src/gone.rs` and no such file exists
- **THEN** a `warn` finding says the file wasn't found

#### Scenario: Name matches several files
- **WHEN** the prose mentions `lib.rs` in backticks and the repository holds `a/src/lib.rs` and `b/src/lib.rs`
- **THEN** a `warn` finding says the name matches 2 files, and never that it wasn't found

#### Scenario: Directory mention
- **WHEN** the prose mentions `tools/sdk-1.4/` or `out/Viewer.app`, and that path is an existing directory
- **THEN** no finding is reported

#### Scenario: Broken relative link
- **WHEN** the prose holds `[setup](docs/setup.md)` and that file does not exist
- **THEN** a `warn` finding is reported
