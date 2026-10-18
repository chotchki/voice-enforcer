# Spec Delta

## Purpose

How voice-enforcer answers Claude Code's PreToolUse hook for file writes, so findings block or warn BEFORE the text lands on disk and Claude gets the reason in the same turn.

## ADDED Requirements

### Requirement: Handled tools

`voice-enforcer hook` SHALL read one PreToolUse hook payload as JSON on stdin and evaluate it only when `tool_name` is `Write`, `Edit` or `MultiEdit`. For any other tool it SHALL print nothing and exit 0.

#### Scenario: Unhandled tool passes through
- **WHEN** the payload's `tool_name` is `Bash`
- **THEN** stdout is empty and the exit code is 0

#### Scenario: Write is evaluated
- **WHEN** the payload's `tool_name` is `Write` with `tool_input.file_path` pointing at a `.md` file
- **THEN** the content is evaluated against the ruleset

### Requirement: Post-tool text is rebuilt before evaluation

The hook SHALL evaluate the file as it will read after the tool runs: for `Write` the `content` field; for `Edit` the current on-disk file with `old_string` replaced by `new_string` (every occurrence when `replace_all` is true, otherwise the single occurrence, and an empty `old_string` creating a file that doesn't exist yet); for `MultiEdit` each edit applied in order to the result of the previous one.

#### Scenario: Edit is evaluated against the edited file
- **WHEN** an Edit replaces "the cache" with "the shiny new cache" in a file on disk
- **THEN** the rules see the full file with the replacement applied

#### Scenario: MultiEdit applies edits in order
- **WHEN** a MultiEdit's second edit targets text introduced by its first edit
- **THEN** the rules see the text with both edits applied

### Requirement: Unrebuildable edits pass through

When the post-tool text can't be rebuilt (the file is unreadable or not UTF-8, `old_string` is absent, or it occurs more than once without `replace_all`) the hook SHALL print nothing and exit 0, leaving the failure for the tool itself to report.

#### Scenario: Ambiguous edit passes through
- **WHEN** an Edit's `old_string` occurs twice in the file and `replace_all` is false
- **THEN** stdout is empty and the exit code is 0

### Requirement: Only changed lines are judged

The hook SHALL report a finding only when the finding's lines intersect a line that is added or changed relative to the current on-disk file. When the file does not exist yet every line counts as changed.

#### Scenario: Pre-existing violation on an untouched line
- **WHEN** a file already contains a banned phrase on line 3 and an Edit changes only line 10
- **THEN** no finding is reported for line 3

#### Scenario: Touching a line makes its violations count
- **WHEN** an Edit rewrites line 3 and the banned phrase survives the rewrite
- **THEN** the finding for line 3 is reported

#### Scenario: New file is judged whole
- **WHEN** a Write creates a file that does not exist on disk
- **THEN** findings on every line are reported

### Requirement: Blocking findings deny the write

When at least one reported finding has severity `block`, the hook SHALL print a JSON object whose `hookSpecificOutput` has `hookEventName` `PreToolUse`, `permissionDecision` `deny` and a `permissionDecisionReason` listing every reported finding, block and warn alike, and SHALL exit 0.

#### Scenario: Blocking finding denies the write
- **WHEN** the changed text contains a phrase from a `block` rule
- **THEN** stdout carries `permissionDecision: "deny"` with the finding in the reason, and the exit code is 0

### Requirement: Warnings ride along as context

When every reported finding is a `warn`, the hook SHALL print a `hookSpecificOutput` with `hookEventName` `PreToolUse` and an `additionalContext` listing them, with no `permissionDecision`, and SHALL exit 0.

#### Scenario: Warnings ride along without blocking
- **WHEN** the only findings come from `warn` rules
- **THEN** stdout carries `additionalContext` with the findings and no `permissionDecision`

### Requirement: Clean writes are silent

With no reported findings the hook SHALL print nothing and exit 0.

#### Scenario: Clean write is silent
- **WHEN** the changed text triggers no rule
- **THEN** stdout is empty and the exit code is 0

### Requirement: The hook never grants permission

The hook SHALL NOT emit `permissionDecision` `allow` or `ask` under any circumstance, so it can never skip or replace the user's own permission prompt.

#### Scenario: Clean write keeps the normal permission flow
- **WHEN** a write produces no findings
- **THEN** no `permissionDecision` is emitted and Claude Code's normal permission handling decides

### Requirement: Finding text

Each finding in a reason or context SHALL state the rule id, the severity, the file path and line, the matched text, the rule's message and the rule's suggestion when it has one, so Claude can fix it without reading the ruleset.

#### Scenario: Finding names everything needed to fix it
- **WHEN** a phrase rule `no-leverage` with suggestion "use" fires on line 4 of `README.md`
- **THEN** the finding text contains `no-leverage`, `block`, `README.md:4`, the matched word, the message and "use"

### Requirement: Fail open

A broken enforcer SHALL NOT block writes. When stdin is not a valid hook payload, or an internal error occurs, the hook SHALL describe the problem on stderr and exit 1 (a non-blocking error to Claude Code) and SHALL NOT exit 2. When a ruleset exists but fails to load, the hook SHALL exit 0 with an `additionalContext` naming the load error. When no ruleset exists the hook SHALL print nothing and exit 0.

#### Scenario: Garbage on stdin
- **WHEN** stdin is not JSON
- **THEN** the exit code is 1, stderr explains why and nothing is denied

#### Scenario: Broken ruleset is surfaced, not enforced
- **WHEN** `ruleset.toml` has an invalid regex
- **THEN** the write is not denied and `additionalContext` names the rule and the error

#### Scenario: No ruleset configured
- **WHEN** no `ruleset.toml` exists at the configured location
- **THEN** stdout is empty and the exit code is 0

### Requirement: Loop guard

After a configurable number of consecutive denies (default 3) for the same file path within the same `session_id`, the next evaluation of that file with blocking findings SHALL report them through `additionalContext` instead of denying, and SHALL say the loop guard tripped. An evaluation of that file with no blocking findings SHALL reset its count.

#### Scenario: Fourth try gets through
- **WHEN** three Writes to the same file in one session were denied and a fourth still has a blocking finding
- **THEN** the fourth is not denied and `additionalContext` lists the findings and says the loop guard tripped

#### Scenario: A clean write resets the count
- **WHEN** two denies are followed by a write to that file with no blocking findings
- **THEN** the next blocking finding on that file is denied again

#### Scenario: Sessions are independent
- **WHEN** a file hit the deny limit in one session and is written in a different session
- **THEN** the blocking finding is denied

### Requirement: Loop guard state

Guard state SHALL live under the user's state directory (`$XDG_STATE_HOME/voice-enforcer`, default `~/.local/state/voice-enforcer`), and state for sessions untouched for 7 days SHALL be deleted.

#### Scenario: Stale sessions are pruned
- **WHEN** the hook runs and a session's state was last touched 8 days ago
- **THEN** that session's state is deleted
