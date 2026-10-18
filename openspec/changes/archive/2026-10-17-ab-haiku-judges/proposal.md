# Proposal

## Why

The deterministic rules catch the tells a phrase list can name. The ones that do the most damage can't be named that way: a recap closer, editorializing, a hidden actor, a verdict buried under its windup, a comment that just restates the line below it. Those need judgment, and Haiku is cheap and fast enough (about 4.5s and $0.0005 per call, measured) to run as a second layer on every write that gets past the first.

## What Changes

- Judges defined in the ruleset (`[[judge]]`): an id, a severity, the registers they read, a scope and a rubric (inline or a file next to `ruleset.toml`). Global `[judges]` settings cover the model (default `haiku`), effort (default `medium`), the command to run and the per-call timeout.
- Two scopes. A **diff** judge sees only the changed prose. A **whole** judge sees all the file's prose with the changed parts marked, for problems that need the rest of the document (repetition). Either way findings only count on changed lines, same as the deterministic rules.
- Ordering: deterministic rules run first, and the judges run only when nothing deterministic blocks (Claude is about to rewrite that text anyway). Each group runs in parallel internally. No changed prose means no judge calls at all, so most code edits cost nothing.
- Each judge is one `claude -p --safe-mode` call with tools off, a fixed harness prompt plus the rubric as the system prompt and a JSON schema for the verdict. `--safe-mode` drops hooks, CLAUDE.md, plugins and MCP but keeps normal subscription auth, which is what `--bare` couldn't do. An env var on the child makes the hook exit immediately if it somehow fires anyway.
- A finding whose quote isn't actually in the judged text gets dropped, so a hallucinated quote can't block a write.
- Fail open: a timeout, a crash, an error verdict or a missing `claude` comes back as a note in `additionalContext`, never a deny.
- A verdict cache keyed on the judge, its rubric, the model settings and the exact input, so re-judging identical text is free.
- Comment spans carry the code that follows them, so a judge can tell when a comment restates the code.
- `check --judges` runs the judges too (off by default, CI usually has no Claude auth). `install` raises the hook timeout to 120s to fit judge calls.
- The example ruleset gains two neutral judges with inline rubrics.

## Capabilities

### New Capabilities

- `llm-judges`: judge configuration, when judges run and on what, the call contract, quote verification, failure handling, the verdict cache and `check --judges`

### Modified Capabilities

None. The aa change's capabilities aren't archived yet (its soak is held until these judges land), so everything here is a new capability and the two changes can archive in either order.

## Non-goals

- Disputes (marking a judge wrong, feeding the samples back as counter-examples): the next change.
- A call budget or usage log. Haiku is cheap; the skip rules and the cache cover the time and cost savings worth having now.
- Judges in CI by default.

## Impact

- `docs/design.md` owns the architecture and gains a judges section; `README.md` moves judges from "planned" to "does today".
- Reach: CI on every push; users pick it up on their next `cargo install`. Existing users need to re-run `install` to get the 120s timeout (it replaces the old entry in place).
- One-way door: `[[judge]]` and `[judges]` join the ruleset format. They're additive, so `version` stays 1 and existing rulesets keep loading; an older binary rejects a ruleset that uses them (strict validation, on purpose).
- New dependencies: `sha2` (cache keys) and `wait-timeout` (killing a judge that runs long).
