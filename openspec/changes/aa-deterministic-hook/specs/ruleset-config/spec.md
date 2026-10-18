# Spec Delta

## Purpose

Where voice-enforcer finds its ruleset, the shape of that ruleset and how bad configuration is reported. Every voice lives here, outside the binary, so the tool stays shareable.

## ADDED Requirements

### Requirement: Ruleset location

voice-enforcer SHALL read `ruleset.toml` from a ruleset directory chosen in this order: the `--ruleset <dir>` flag (for `check` and `validate`), then the `VOICE_ENFORCER_RULESET` environment variable, then `$XDG_CONFIG_HOME/voice-enforcer`, then `~/.config/voice-enforcer`. The `~/.config` default SHALL hold on macOS too.

#### Scenario: Environment variable wins over the default
- **WHEN** `VOICE_ENFORCER_RULESET` points at a directory holding a `ruleset.toml`
- **THEN** that ruleset is used and `~/.config/voice-enforcer` is not read

#### Scenario: Flag wins over the environment
- **WHEN** `check --ruleset ./a` runs with `VOICE_ENFORCER_RULESET=./b`
- **THEN** `./a/ruleset.toml` is used

### Requirement: Versioned format

`ruleset.toml` SHALL declare `version = 1`. A missing `version` or any other value SHALL be a load error that names the versions this binary supports.

#### Scenario: Future ruleset on an old binary
- **WHEN** `ruleset.toml` declares `version = 2`
- **THEN** loading fails with an error naming version 1 as supported

### Requirement: Strict validation

Loading SHALL fail on unknown keys, duplicate rule ids, invalid regexes (`pattern` or `allow_when`), invalid globs, an unknown `kind`, `severity` or `register`, and a sentence limit below 1. The error SHALL name the offending rule id or key and its line in `ruleset.toml`.

#### Scenario: Typo in a key
- **WHEN** a rule spells `severity` as `severty`
- **THEN** loading fails and the error names `severty` and its line

#### Scenario: Duplicate id
- **WHEN** two rules share the id `no-hype`
- **THEN** loading fails and the error names `no-hype`

### Requirement: Configured excludes

A file whose absolute path matches a glob in the ruleset's `exclude` list, with a leading `~` expanded to the home directory, SHALL yield no prose.

#### Scenario: Someone else's repo
- **WHEN** `exclude` holds `~/workspace/their-repo/**` and a file under it is written
- **THEN** no rule produces a finding

### Requirement: Built-in excludes

Built-in excludes SHALL cover `Cargo.lock`, `package-lock.json`, `yarn.lock`, `pnpm-lock.yaml`, `Gemfile.lock`, `poetry.lock`, `*.min.js` and any path inside a `target`, `node_modules`, `vendor`, `dist` or `.git` directory, and a matching file SHALL yield no prose. `build` is deliberately NOT built in, since plenty of repos keep hand-written scripts there.

#### Scenario: Built-in exclude
- **WHEN** a file under `node_modules/` is written
- **THEN** no rule produces a finding

#### Scenario: build is not excluded
- **WHEN** a file under `build/` is written
- **THEN** the rules run on it

### Requirement: Extra comment directives

The ruleset's `comment_directives` list SHALL add regexes to the built-in tool directive patterns, and a comment whose text matches one SHALL NOT be prose.

#### Scenario: Custom directive
- **WHEN** `comment_directives` holds `^lint:allow` and a comment reads `lint:allow long-line`
- **THEN** that comment is not prose

### Requirement: Loop guard setting

The ruleset's `loop_guard.max_consecutive_blocks` SHALL set the deny limit used by the hook's loop guard. It SHALL default to 3 and SHALL be at least 1.

#### Scenario: Tighter guard
- **WHEN** `loop_guard.max_consecutive_blocks = 1`
- **THEN** the second consecutive evaluation of a file with a blocking finding in one session warns instead of denying

### Requirement: Example ruleset

The repository SHALL ship an example ruleset at `examples/ruleset/ruleset.toml` that passes `validate`, uses at least one rule of every kind and holds only synthetic text, never text quoted from a real voice profile.

#### Scenario: Example stays valid
- **WHEN** `voice-enforcer validate --ruleset examples/ruleset` runs in CI
- **THEN** it exits 0
