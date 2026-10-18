# Spec Delta

## Purpose

Haiku judges as a second layer after the deterministic rules: the judgment-call tells (recaps, editorializing, a comment restating the code) that no phrase list can name, run cheaply and failing open.

## ADDED Requirements

### Requirement: Judge configuration

A `[[judge]]` table SHALL hold a unique `id` (also distinct from every rule id), a `severity` of `block` or `warn`, a `scope` of `diff` or `whole`, optional `registers` (default both) and exactly one of `rubric` (inline text) or `rubric_file` (a path relative to the ruleset directory). Any violation SHALL be a load error naming the judge id and its line.

#### Scenario: Rubric from a file
- **WHEN** a judge sets `rubric_file = "judges/recap.md"` and that file exists next to `ruleset.toml`
- **THEN** the judge's rubric is that file's text

#### Scenario: Both rubric forms
- **WHEN** a judge sets both `rubric` and `rubric_file`
- **THEN** loading fails and the error names the judge

#### Scenario: Missing rubric file
- **WHEN** `rubric_file` names a file that doesn't exist
- **THEN** loading fails and the error names the judge and the path

### Requirement: Global judge settings

A `[judges]` table SHALL accept `model` (default `haiku`), `effort` (default `medium`), `command` (default `claude`) and `timeout_secs` (default 45, at least 1), and these SHALL apply to every judge.

#### Scenario: Defaults
- **WHEN** a ruleset defines judges and no `[judges]` table
- **THEN** each judge call uses model `haiku`, effort `medium`, the `claude` command and a 45 second timeout

### Requirement: Judges run only when nothing deterministic blocks

The hook SHALL run the judges after the deterministic rules, and SHALL NOT run them when any reported deterministic finding is a `block`.

#### Scenario: Deterministic block skips judges
- **WHEN** a write has a blocking phrase finding on a changed line
- **THEN** no judge is called and the write is denied for the phrase alone

#### Scenario: Deterministic warnings still get judged
- **WHEN** a write's only deterministic findings are warnings
- **THEN** the judges run and their findings join the warnings

### Requirement: No changed prose means no judge calls

A judge SHALL be called only when at least one prose span in a register it reads has a changed line. Otherwise it SHALL NOT be called.

#### Scenario: Code-only edit
- **WHEN** an Edit changes a Rust statement and no comment
- **THEN** no judge is called

#### Scenario: Register filter
- **WHEN** a judge reads only `long_form` and a write changes only a code comment
- **THEN** that judge is not called

### Requirement: What a diff judge sees

A `diff` judge SHALL receive only the prose spans that have a changed line, each labeled with its line range and register (and level for comments).

#### Scenario: Untouched paragraphs stay out
- **WHEN** an Edit changes one paragraph of a ten-paragraph README
- **THEN** a diff judge's input holds that paragraph and none of the other nine

### Requirement: What a whole judge sees

A `whole` judge SHALL receive every prose span in the file in a register it reads, with the spans that have a changed line marked as changed.

#### Scenario: Repetition needs the rest of the file
- **WHEN** an Edit adds a paragraph that repeats one written earlier in the file
- **THEN** a whole judge's input holds both paragraphs with only the new one marked changed

### Requirement: Judges see real text and the code under a comment

Judge input SHALL use the unmasked text (inline code, URLs and inline HTML as written). A comment span SHALL be followed in the input by up to 20 source lines after the comment block, labeled as code context and never judged.

#### Scenario: Comment restating code
- **WHEN** a changed comment reads "increment the counter" above `count += 1`
- **THEN** the judge input holds the comment followed by the `count += 1` line as code context

### Requirement: Findings only on changed lines

A judge finding SHALL be reported only when one of its lines (the quote's first through last line) is a changed line, for both scopes, the same test the deterministic rules use.

#### Scenario: Whole judge quotes old text
- **WHEN** a whole judge quotes a sentence on a line the write didn't change
- **THEN** that finding is dropped

### Requirement: Judge call contract

Each judge SHALL run as `<command> -p --safe-mode --model <model> --effort <effort> --tools "" --no-session-persistence --output-format json --json-schema <schema> --system-prompt <prompt>`, with the judge input on stdin and `VOICE_ENFORCER_JUDGE=1` in its environment. The system prompt SHALL be the fixed judge instructions followed by the rubric.

#### Scenario: Flags and environment
- **WHEN** a judge runs
- **THEN** the command receives every flag above, the input on stdin and `VOICE_ENFORCER_JUDGE=1`

### Requirement: Recursion guard

When `VOICE_ENFORCER_JUDGE` is set in its environment, `voice-enforcer hook` SHALL print nothing and exit 0 without reading the ruleset.

#### Scenario: Hook inside a judge
- **WHEN** the hook runs with `VOICE_ENFORCER_JUDGE=1`
- **THEN** stdout is empty and the exit code is 0

### Requirement: Judges run in parallel

All judges called for one write SHALL run concurrently, so a write waits for the slowest judge, not the sum.

#### Scenario: Two slow judges
- **WHEN** two judges each take 2 seconds
- **THEN** the hook finishes in well under 4 seconds

### Requirement: Verdicts become findings

A judge verdict SHALL be a list of findings, each with a `quote`, an `explanation` and an optional `suggestion`. A finding whose quote doesn't appear in the judge's input, ignoring whitespace differences, SHALL be dropped. A kept finding SHALL carry the judge id as its rule id, the judge's severity, the line where the quote starts, the quote as matched text, the explanation as message and the suggestion.

#### Scenario: Hallucinated quote
- **WHEN** a judge returns a quote that appears nowhere in its input
- **THEN** no finding is reported for it

#### Scenario: Quote wrapped across lines
- **WHEN** the quoted sentence is wrapped across two lines in the file
- **THEN** the finding is kept and reports the first line

### Requirement: Judge findings flow through the normal decision

Judge findings SHALL join the deterministic findings in the same deny / `additionalContext` / silent output and SHALL count toward the loop guard like any other blocking finding.

#### Scenario: Blocking judge
- **WHEN** a `block` judge reports a finding on a changed line
- **THEN** the write is denied and the reason names the judge id and the quote

### Requirement: Judge failures fail open

When a judge times out (and is killed), exits non-zero, reports an error, returns output that doesn't parse or can't be started, it SHALL contribute no findings and the hook SHALL add a note to `additionalContext` naming the judge and the reason. A judge failure SHALL NEVER cause a deny.

#### Scenario: Timeout
- **WHEN** a judge runs past `timeout_secs`
- **THEN** it is killed, the write is not denied on its account and `additionalContext` says the judge timed out

#### Scenario: Missing command
- **WHEN** `command` isn't on `PATH`
- **THEN** the write is not denied and `additionalContext` says the judge couldn't start

### Requirement: Verdict cache

A verdict SHALL be cached under `$XDG_CACHE_HOME/voice-enforcer` (default `~/.cache/voice-enforcer`) keyed on the judge id, its rubric, the model, the effort and the exact input, and an identical later call SHALL use the cached verdict without running the command. Failed calls SHALL NOT be cached. Entries unused for 30 days SHALL be deleted.

#### Scenario: Identical input
- **WHEN** the same judge sees exactly the same input twice
- **THEN** the command runs once

#### Scenario: Rubric change busts the cache
- **WHEN** a judge's rubric changes between two otherwise identical calls
- **THEN** the command runs both times

### Requirement: check runs judges on request

`voice-enforcer check` SHALL run judges only when given `--judges`, judging each file whole with every span treated as changed, after the same deterministic-block skip as the hook.

#### Scenario: Default check skips judges
- **WHEN** `check README.md` runs with a ruleset that defines judges
- **THEN** no judge is called

#### Scenario: Opted in
- **WHEN** `check --judges README.md` runs and README.md has no blocking deterministic finding
- **THEN** every judge reading long-form prose is called once for README.md
