# Spec Delta

## Purpose

The rule kinds a ruleset can configure, how each one matches prose and what a finding carries. These run before any LLM judge, and they're exact and free, so the known tells never cost a model call.

## ADDED Requirements

### Requirement: Common rule fields

Every rule SHALL have a unique `id`, a `kind`, a `severity` of `block` or `warn`, a `message` and optionally a `suggestion`. A rule MAY set `registers` to any of `long_form` and `comment` (default both), and SHALL run only on prose in those registers.

#### Scenario: Register scoping
- **WHEN** a rule has `registers = ["long_form"]` and its phrase appears only in a code comment
- **THEN** no finding is reported

### Requirement: Phrase rules

A `phrase` rule SHALL match each listed phrase case-insensitively, as whole words, with any run of whitespace (line breaks included) in the prose matching a single space in the phrase. Every match SHALL be a finding. When any regex in the rule's `allow_when` list matches the sentence containing a match, that match SHALL be dropped.

#### Scenario: Case-insensitive whole-word hit
- **WHEN** the phrase list holds "leverage" and the prose reads "We Leverage the cache."
- **THEN** one finding is reported for "Leverage"

#### Scenario: Partial words do not match
- **WHEN** the phrase list holds "leverage" and the prose reads "the leveraged buyout"
- **THEN** no finding is reported

#### Scenario: Phrase wrapped across lines
- **WHEN** the phrase list holds "at the end of the day" and markdown wraps it after "end"
- **THEN** one finding is reported

#### Scenario: Allow-when exception
- **WHEN** the phrase list holds "seam", `allow_when` holds `test seam` and the prose reads "add a test seam here"
- **THEN** no finding is reported

### Requirement: Regex rules

A `regex` rule SHALL report a finding for every non-overlapping match of its `pattern` in the prose.

#### Scenario: Pattern match
- **WHEN** the pattern is `(?i)\bvery (unique|important)\b` and the prose reads "a very important step"
- **THEN** one finding is reported for "very important"

### Requirement: Oxford comma rule

An `oxford_comma` rule SHALL report a finding for a comma placed directly before the final `and` or `or` of a list, where a list is at least two earlier comma-separated items in the same sentence, each at most `max_item_words` words long (default 4). A comma before `and` or `or` with no list before it in the sentence SHALL NOT be a finding.

#### Scenario: Serial comma flagged
- **WHEN** the prose reads "red, green, and blue"
- **THEN** one finding is reported on the comma before "and"

#### Scenario: No serial comma
- **WHEN** the prose reads "red, green and blue"
- **THEN** no finding is reported

#### Scenario: Two independent clauses
- **WHEN** the prose reads "I fixed the bug, and the build passes now."
- **THEN** no finding is reported

### Requirement: Comment sentence limits

A `sentence_limit` rule SHALL apply only to the comment register and SHALL hold a `max_sentences` limit for any of the `line`, `item` and `module` levels. A comment block with more sentences than the limit for its level SHALL produce one finding stating the count and the limit, on the block's first line.

#### Scenario: Doc comment over its limit
- **WHEN** the `item` limit is 2 and a `///` block above a function holds three sentences
- **THEN** one finding reports 3 sentences against a limit of 2

#### Scenario: Level without a limit
- **WHEN** the rule sets limits only for `line` and `item`
- **THEN** module-level comments are never reported by it

### Requirement: Sentence boundaries

A sentence SHALL end at `.`, `!` or `?` followed by whitespace or the end of the block, with capitalization never a signal. Common abbreviations (`e.g.`, `i.e.`, `etc.`, `vs.`), decimals, version numbers, file names, masked inline code and a number opening a line (`1.`) SHALL NOT end a sentence. A block with no terminal punctuation SHALL count as one sentence.

#### Scenario: Abbreviations and versions don't split
- **WHEN** a line comment reads "Pins v1.2.3 of the client, e.g. for retries."
- **THEN** it counts as one sentence

#### Scenario: Lowercase starts still split
- **WHEN** a comment reads "first thing. second thing."
- **THEN** it counts as two sentences

### Requirement: File mentions

A `file_links` rule SHALL apply only to long-form markdown. A file mention SHALL be a token that is not the text of a link and is either a path holding a `/` and a file extension, a bare file name inside inline code whose extension is a common file extension (`md`, `rs`, `toml`, `json`, `py`, `ts` and the like), or a bare file name with an extension that resolves to an existing file. URLs, absolute paths and `~` paths SHALL NOT be mentions.

#### Scenario: Product names stay quiet
- **WHEN** the prose mentions Node.js in plain text and no file named `Node.js` exists
- **THEN** no finding is reported

#### Scenario: Code in backticks stays quiet
- **WHEN** the prose holds `` `self.size` `` and no file named `self.size` exists
- **THEN** no finding is reported

### Requirement: Mention resolution

A mention SHALL resolve relative to the markdown file's directory, then the repository root (the git top-level), then by a unique match anywhere in the repository outside excluded and git-ignored paths, where a bare name matches a file name and a path matches the end of a file's repository path.

#### Scenario: Bare name found elsewhere in the repo
- **WHEN** the root README.md mentions `guide.md` and the only file with that name is `docs/guide.md`
- **THEN** a finding at the rule's severity suggests a link to `docs/guide.md`

### Requirement: File link findings

A mention that resolves SHALL be a finding at the rule's severity, with a suggestion holding the relative link from the markdown file to the resolved file. A path or inline-code mention that does not resolve SHALL be a `warn` finding saying the named file wasn't found, whatever the rule's severity. A relative link whose target does not exist SHALL be a `warn` finding.

#### Scenario: Unlinked existing file
- **WHEN** `docs/guide.md` mentions `src/main.rs` in backticks, not as a link, and `src/main.rs` exists at the repository root
- **THEN** a finding at the rule's severity suggests `[src/main.rs](../src/main.rs)`

#### Scenario: Linked file is fine
- **WHEN** the prose holds `[src/main.rs](src/main.rs)` and the file exists
- **THEN** no finding is reported

#### Scenario: Named file is missing
- **WHEN** the prose mentions `src/gone.rs` and no such file exists
- **THEN** a `warn` finding says the file wasn't found

#### Scenario: Broken relative link
- **WHEN** the prose holds `[setup](docs/setup.md)` and that file does not exist
- **THEN** a `warn` finding is reported

### Requirement: Finding contents

Every finding SHALL carry the rule id, the severity, the 1-based line where the match starts, the matched text, the rule's message and the suggestion when there is one.

#### Scenario: Complete finding
- **WHEN** any rule fires
- **THEN** the finding has every field above filled in
