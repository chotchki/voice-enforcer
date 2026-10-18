# Proposal

## Why

A voice profile only works while it's in the context window, and a long session pushes a big markdown file out of it (the drift starts quietly, nobody notices until a README reads like a press release). Enforcement has to live OUTSIDE the context: something that runs on every write no matter what the model remembers. This change lands the foundation, a Rust binary wired as a Claude Code hook that blocks or warns on the mechanical tells, with no LLM in the loop yet (the Haiku judges are the next change and build on this one).

## What Changes

- New `voice-enforcer` binary, installed with `cargo install`. It's voice-AGNOSTIC: every rule comes from a ruleset directory outside this repo, so the tool can be shared without sharing anyone's profile.
- `voice-enforcer hook` runs as a PreToolUse hook on Write, Edit and MultiEdit. It rebuilds the file as it will look after the tool runs, extracts the prose, runs the rules against the lines that changed and answers Claude Code with a deny (any blocking finding) or extra context (warnings only). Nothing bad lands on disk, Claude gets the reason and rewrites.
- Prose extraction: markdown body as long-form (code blocks skipped, inline code masked), plain `.txt` as long-form and tree-sitter comment nodes for Rust, Swift, TypeScript/JavaScript, Python and shell, each classified as a line, item or module comment.
- Deterministic rule kinds, all configured from the ruleset with a per-rule severity (block or warn) and register scope:
  - banned phrases with allow-when exceptions (the bulk of the AI-tell killing)
  - raw regex
  - Oxford comma
  - comment sentence limits per level (line / item / module)
  - file links: a named file must be a link, a named file that can't be found is a warn
- Loop guard: after N consecutive blocks on the same file in one session the blocks downgrade to warnings and say so, so a bad rule can't wedge a session.
- CLI verbs beside `hook`: `check` (run the rules on files, for CI or by hand), `validate`, `init` (write the example ruleset) and `install` / `uninstall` (merge the hook into `~/.claude/settings.json`, backup first).
- A neutral example ruleset with synthetic prose, GitHub Actions CI with a 99% line-coverage gate whose summary lands in the job summary and the stock MIT OR Apache-2.0 license.

## Capabilities

### New Capabilities

- `hook-intake`: the PreToolUse contract - which tools it handles, how the post-tool text is rebuilt, the diff scoping, the deny / context / pass decision, fail-open behavior and the loop guard
- `prose-extraction`: which files yield prose, the long-form vs comment registers, comment level classification and what gets skipped (code blocks, generated files, directives)
- `deterministic-rules`: the rule kinds, their matching semantics, severity and register scoping plus the finding format
- `ruleset-config`: where the ruleset is found, its versioned TOML format, validation and path excludes
- `cli`: the `check`, `validate`, `init`, `install` and `uninstall` verbs and their exit codes

### Modified Capabilities

None (specs start empty with this change).

## Non-goals

- Haiku judges, the verdict cache, the recursion guard and dispute samples: change ab.
- The `gh` wrapper, commit and PR bodies and file ownership (whose file is this, do I restyle it): change ac. Until then a path exclude in the ruleset is the manual answer for repos that aren't mine to restyle.
- docx / odt: change ad, once we know how those writes can be intercepted (Claude usually builds them through Bash, which a Write/Edit matcher never sees).
- NotebookEdit and more tree-sitter languages: backlog.

## Impact

- Greenfield: the Cargo crate, `.github/workflows/ci.yml`, `docs/design.md` (created here, owns the architecture from now on) and `README.md` (the map).
- Reach: CI on every push; users pick it up on their next `cargo install`. `install` is the only thing that touches `~/.claude/settings.json` and it's an explicit verb.
- One-way door: the ruleset format is the contract with every private ruleset, and those live where this repo can't see them. The format carries a `version` field from day one so a later change can detect an old ruleset and say what to migrate instead of silently misreading it.
- New dependencies: `tree-sitter` plus the six grammars, `pulldown-cmark`, `serde` / `toml` / `serde_json`, `regex`, `similar` (line diff), `globset` and `clap`.
