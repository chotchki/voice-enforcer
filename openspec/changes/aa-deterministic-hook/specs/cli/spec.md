# Spec Delta

## Purpose

The commands a person runs directly: checking files by hand or in CI, validating a ruleset and wiring the hook into Claude Code without hand-editing its settings.

## ADDED Requirements

### Requirement: check evaluates whole files

`voice-enforcer check <path>...` SHALL evaluate each file whole (not diff-scoped) through the same pipeline as the hook, and SHALL walk directories, skipping git-ignored and excluded files.

#### Scenario: Clean tree
- **WHEN** `check .` runs over files that trigger no rule
- **THEN** nothing is printed and the exit code is 0

### Requirement: check output

`check` SHALL print one line per finding as `<path>:<line>: <severity> <rule-id>: <message> ("<matched text>")`, followed by an indented suggestion line when there is one, or a JSON array of findings with `--format json`.

#### Scenario: JSON output
- **WHEN** `check --format json README.md` finds two problems
- **THEN** stdout is a JSON array of two findings carrying path, line, severity, rule id, matched text, message and suggestion

### Requirement: check exit codes

`check` SHALL exit 0 when no finding is a `block`, 1 when any is and 2 on a usage error, an unreadable path, a missing ruleset or a ruleset that fails to load.

#### Scenario: Blocking finding
- **WHEN** a checked file triggers a `block` rule
- **THEN** the finding is printed and the exit code is 1

#### Scenario: Warnings alone pass
- **WHEN** the only findings are `warn`
- **THEN** they are printed and the exit code is 0

#### Scenario: Missing path
- **WHEN** `check missing.md` runs and the file doesn't exist
- **THEN** the exit code is 2

### Requirement: validate

`voice-enforcer validate` SHALL load the ruleset and exit 0 printing the rule count when it is valid, and exit 2 printing the error when it is invalid or missing.

#### Scenario: Valid ruleset
- **WHEN** the ruleset loads cleanly with 12 rules
- **THEN** the output mentions 12 rules and the exit code is 0

#### Scenario: Missing ruleset
- **WHEN** no `ruleset.toml` exists at the resolved location
- **THEN** the output names the path it looked at and the exit code is 2

### Requirement: init

`voice-enforcer init` SHALL write the example ruleset into the resolved ruleset directory, creating the directory if needed. When a `ruleset.toml` already exists there it SHALL write nothing and exit 2, unless `--force` is given.

#### Scenario: First run
- **WHEN** `init` runs with no ruleset present
- **THEN** `ruleset.toml` is written and `validate` passes on it

#### Scenario: Existing ruleset is protected
- **WHEN** `init` runs and a `ruleset.toml` exists
- **THEN** the file is unchanged and the exit code is 2

### Requirement: install

`voice-enforcer install` SHALL add a PreToolUse hook entry to `~/.claude/settings.json` (or the file named by `--settings <path>`), with matcher `Write|Edit|MultiEdit` and the absolute path of the running binary followed by `hook` as the command, creating the file when it does not exist.

#### Scenario: Fresh settings
- **WHEN** `install` runs and no settings file exists
- **THEN** the file is created holding the voice-enforcer entry

### Requirement: install preserves settings

`install` SHALL keep every other key and hook in the file unchanged, write a timestamped backup next to the file before changing it and leave exactly one voice-enforcer entry no matter how many times it runs.

#### Scenario: Existing settings survive
- **WHEN** `install` runs against a settings file holding permissions and a Notification hook
- **THEN** both are still present afterwards beside the new PreToolUse entry, and a backup of the original exists

#### Scenario: Idempotent
- **WHEN** `install` runs twice
- **THEN** the settings file holds one voice-enforcer entry

### Requirement: install dry run

With `--dry-run`, `install` SHALL print the resulting JSON and write nothing.

#### Scenario: Dry run writes nothing
- **WHEN** `install --dry-run` runs
- **THEN** the resulting JSON is printed and the settings file and its directory are unchanged

### Requirement: uninstall

`voice-enforcer uninstall` SHALL remove only hook entries whose command ends in `voice-enforcer hook`, write a timestamped backup first and keep everything else in the file unchanged. When no such entry exists it SHALL change nothing and exit 0.

#### Scenario: Only our entry goes
- **WHEN** `uninstall` runs against settings holding our entry and another PreToolUse hook
- **THEN** only our entry is removed

#### Scenario: Nothing to remove
- **WHEN** `uninstall` runs and no voice-enforcer entry exists
- **THEN** the settings file is unchanged and the exit code is 0
