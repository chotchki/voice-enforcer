# Spec Delta

## Purpose

Lets Claude mark a rule or judge finding as wrong and records the evidence (the finding, the prose around it and the reason) in the ruleset directory, so the ruleset owner can tune rules against real misfires. Recording is all it does.

## ADDED Requirements

### Requirement: Findings carry a dispute id

Every finding listed in the hook's deny reason or `additionalContext` SHALL show a dispute id of 8 lowercase hex characters. The same session, file, rule or judge id and matched text SHALL always produce the same id, so a finding keeps its id across retries even when its line moves.

#### Scenario: Id in the listing
- **WHEN** a write is denied for a phrase finding
- **THEN** the finding's line in the reason includes `(id ` followed by 8 hex characters and `)`

#### Scenario: Stable across retries
- **WHEN** the same text is written twice in one session and blocked both times, once on line 3 and once on line 5
- **THEN** both listings show the same id for it

### Requirement: Dispute hint

When the hook lists at least one finding, its output SHALL end with one line giving the command to dispute a finding (using the absolute path of the running binary) and saying that a dispute records the finding for the ruleset owner without changing this write.

#### Scenario: Hint after the findings
- **WHEN** the hook denies a write
- **THEN** the reason's last line names `dispute <id> --reason` and says the write is unchanged by it

### Requirement: Samples are kept for disputes

For every finding the hook lists, it SHALL save a sample under `$XDG_STATE_HOME/voice-enforcer/findings` (default `~/.local/state/voice-enforcer/findings`) holding the rule or judge id, whether it came from a rule or a judge, the severity, the file, the line, the matched text, the message, the suggestion, the prose span around the match and that span's code context. A judge sample SHALL also hold the model, the effort and a hash of the rubric.

#### Scenario: Judge sample
- **WHEN** a judge finding is listed
- **THEN** its sample names the judge, the model, the effort and the rubric hash

### Requirement: Sample upkeep is best effort

Samples untouched for 7 days SHALL be deleted. Saving a sample SHALL NOT change the hook's decision or output, even when it fails.

#### Scenario: Unwritable state
- **WHEN** the state directory can't be written
- **THEN** the hook's decision and output are unchanged

#### Scenario: Old samples expire
- **WHEN** the hook runs and a sample was last touched 8 days ago
- **THEN** that sample is deleted

### Requirement: dispute records the sample

`voice-enforcer dispute <id> --reason "<why>"` SHALL write the saved sample for `<id>` plus the reason, the dispute time and `version = 1` as a TOML file at `disputes/<rule-or-judge-id>/<UTC timestamp>-<id>.toml` inside the resolved ruleset directory (honoring `--ruleset` like `check`), print the path and exit 0.

#### Scenario: Dispute a judge finding
- **WHEN** `dispute 3fa9c201 --reason "a real contrast, not a recap"` runs for a judge finding from `recap-closer`
- **THEN** a file under `disputes/recap-closer/` holds the quote, the explanation, the surrounding prose and the reason, and the command prints its path

#### Scenario: Dispute a rule finding
- **WHEN** a phrase rule finding is disputed
- **THEN** the file lands under `disputes/<rule id>/` with the matched phrase and the reason

### Requirement: dispute refuses bad input

`dispute` SHALL exit 2 without writing anything when the reason is missing or blank, or when no saved sample matches the id (unknown, mistyped or older than 7 days).

#### Scenario: Blank reason
- **WHEN** `dispute 3fa9c201 --reason "  "` runs
- **THEN** nothing is written and the exit code is 2

#### Scenario: Unknown id
- **WHEN** `dispute deadbeef --reason "wrong"` runs and no sample has that id
- **THEN** nothing is written, the output says the id is unknown or expired and the exit code is 2

### Requirement: Disputing twice writes nothing new

When a dispute file for the id already exists, `dispute` SHALL write nothing, print the existing file's path and exit 0.

#### Scenario: Repeat dispute
- **WHEN** the same id is disputed twice
- **THEN** exactly one dispute file exists for it

### Requirement: Disputes are record-only

A dispute SHALL NOT change how any rule or judge evaluates text: the same write SHALL produce the same findings before and after a dispute.

#### Scenario: Same text after a dispute
- **WHEN** a finding is disputed and the same text is written again
- **THEN** the finding is reported again with the same id
