# Spec Delta

## ADDED Requirements

### Requirement: Per-judge overrides

A `[[judge]]` table MAY set `model` and `effort` (accepting the same values as the `[judges]` table) and `exclude` (a list of globs matched like a rule's `exclude`). A model or effort set on a judge SHALL apply to that judge's calls only. A judge SHALL NOT run on a file matching one of its `exclude` globs. A bad value or glob SHALL be a load error naming the judge and its line.

#### Scenario: One judge at a higher effort
- **WHEN** `[judges]` sets `effort = "medium"` and one judge sets `effort = "high"`
- **THEN** that judge's call gets `--effort high` and every other judge's gets `--effort medium`

#### Scenario: Judge skips a folder
- **WHEN** a judge sets `exclude = ["**/openspec/**/specs/**"]` and a write lands in `openspec/changes/x/specs/a/spec.md`
- **THEN** that judge isn't called for the write and the other judges are

#### Scenario: Unknown effort on a judge
- **WHEN** a judge sets `effort = "extreme"`
- **THEN** loading fails and the error names the judge

## MODIFIED Requirements

### Requirement: Global judge settings

A `[judges]` table SHALL accept `model` (default `haiku`), `effort` (default `medium`), `command` (default `claude`) and `timeout_secs` (default 45, at least 1), and these SHALL apply to every judge, except that a judge's own `model` or `effort` replaces the table's for that judge.

#### Scenario: Defaults
- **WHEN** a ruleset defines judges and no `[judges]` table
- **THEN** each judge call uses model `haiku`, effort `medium`, the `claude` command and a 45 second timeout

### Requirement: Judges see real text and the code under a comment

Judge input SHALL use the unmasked text (inline code, URLs and inline HTML as written). A comment span SHALL be followed in the input by up to 20 source lines after the comment block, labeled as code context and never judged. A comment that follows code on its own line SHALL get that code as the first line of its code context.

#### Scenario: Comment restating code
- **WHEN** a changed comment reads "increment the counter" above `count += 1`
- **THEN** the judge input holds the comment followed by the `count += 1` line as code context

#### Scenario: Trailing comment
- **WHEN** a changed comment `# load word` trails `elif op == 32:` on the same line
- **THEN** the code context opens with `elif op == 32:` and then the lines after it

### Requirement: Findings only on changed lines

A judge finding SHALL be reported only when its quote overlaps a changed word, using the hook's word-level change test. A `diff` judge's finding SHALL also be dropped inside text that already exists at HEAD, as the hook drops deterministic findings; a `whole` judge's SHALL NOT, since a passage copied from elsewhere in the repository is what a whole judge exists to catch.

#### Scenario: Whole judge quotes old text
- **WHEN** a whole judge quotes a sentence on a line the write didn't change
- **THEN** that finding is dropped

#### Scenario: Old sentence in a changed paragraph
- **WHEN** an Edit changes one word in the last sentence of a paragraph and a diff judge quotes the paragraph's first sentence
- **THEN** that finding is dropped

#### Scenario: Copied paragraph
- **WHEN** a Write copies a paragraph that already exists at HEAD into a new section and a whole judge quotes it as a repeat
- **THEN** that finding is reported

### Requirement: Verdicts become findings

A judge verdict SHALL be a list of findings, each with a `quote`, an `explanation`, an optional `suggestion` and a required `violates` flag. A finding whose `violates` is false, or whose quote doesn't appear in the judge's input ignoring whitespace differences, SHALL be dropped. A kept finding SHALL carry the judge id as its rule id, the judge's severity, the line where the quote starts, the quote as matched text, the explanation as message and the suggestion.

#### Scenario: Hallucinated quote
- **WHEN** a judge returns a quote that appears nowhere in its input
- **THEN** no finding is reported for it

#### Scenario: Quote wrapped across lines
- **WHEN** the quoted sentence is wrapped across two lines in the file
- **THEN** the finding is kept and reports the first line

#### Scenario: Judge clears its own quote
- **WHEN** a judge returns a finding with `violates: false` and an explanation saying the text is outside its rubric
- **THEN** no finding is reported for it
