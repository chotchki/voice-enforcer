# Proposal

## Why

Rules and judges will misfire (the judges already flagged a genuine contrast in our own design doc as a recap), and today a misfire leaves no trace: Claude rewrites around it or the loop guard waves it through, and the evidence the ruleset owner needs to fix the rule is gone. A dispute captures that evidence at the moment it exists, so rubrics and phrase lists get tuned against real misfires instead of guesses.

## What Changes

- Every finding in the hook's output gets a short id, stable across retries of the same text, and one line tells Claude how to dispute a finding it thinks is wrong.
- The hook keeps a sample for each finding it reports (rule or judge id, the matched text, the message, the prose around it and, for comments, the code after it) in the state dir for 7 days, so a dispute can recover it.
- New `voice-enforcer dispute <id> --reason "<why>"` writes that sample plus the reason to `disputes/<rule-or-judge-id>/` inside the ruleset directory, one TOML file per dispute. Disputing the same finding twice writes nothing new.
- Record only, by design: a dispute changes NOTHING about how rules and judges behave. The same text gets flagged again next time, and the hint says so, so Claude rewrites (or hits the loop guard) instead of retrying the disputed text. Folding the samples into rubrics and phrase lists is the ruleset owner's job, and the dispute files show up as a diff in whatever repo holds the ruleset.
- Both kinds of finding can be disputed: deterministic rule hits and judge findings.

## Capabilities

### New Capabilities

- `disputes`: finding ids in the hook output, the persisted samples, the `dispute` command and the dispute file format

### Modified Capabilities

None. The hook's finding format lives in aa's `hook-intake`, which isn't archived yet, so the id requirement lands in `disputes` instead of as a delta on a spec that doesn't exist in `openspec/specs/` yet.

## Non-goals

- Automatic feedback: no suppression of disputed text, no counter-examples in judge prompts (chosen against: the owner tunes by hand).
- Ids on `check` output. `check` is for people and CI, and people can fix the ruleset directly.
- A `disputes` listing command. They're plain files in the ruleset dir.

## Impact

- `docs/design.md` gains a disputes section; `README.md` mentions the command.
- Reach: CI on every push; users pick it up on their next `cargo install`.
- One-way door: the dispute file format. Rulesets in private repos will accumulate these files, so the format carries a `version` field like the ruleset does.
- Hook output changes shape (an id per finding plus a hint line), which updates the snapshot tests; nothing downstream parses it besides Claude.
- No new dependencies (`sha2` and `toml` are already in).
