# Backlog

Unphased ideas and tech debt. Promoting one means proposing a change (`/opsx:propose`) and deleting its line here.

- **ac - judge disputes.** A `dispute` verb writes the disputed sample into the ruleset dir and judges get recent disputes back as counter-examples. Claude may run `dispute` itself (it hasn't abused it in practice); the dispute files ARE the audit trail, so a bad call shows up as a diff in the private ruleset repo.
- **ad - gh wrapper and ownership.** Deny direct `gh` in Bash with a PreToolUse rule and route through `voice-enforcer gh ...`, which runs `--title` / `--body` / `--body-file` through the pipeline before exec'ing the real `gh`. Use gh / git authorship to decide whose prose a file is, with a per-repo policy: enforce on everything, or respect the house style of files someone else wrote. Commit messages go through a PreToolUse match on `git commit` in Bash (`-m` / `-F`), never a git `commit-msg` hook, since the tool governs Claude's writes only.
- **ae - docx / odt.** chotchki to check how prior art intercepted these (it may not have been right, so this probably wants a fresh design). Candidates: a Bash PostToolUse that scans for office files changed during the call, or catching the unpack, Edit XML, repack path at the XML Edit.
- More tree-sitter languages (YAML / TOML comments, Go, C / C++, Java / Kotlin, HTML / CSS). Every file getting the treatment is the goal, and aa covers six languages plus markdown.
- NotebookEdit: markdown cells as long-form, code cell comments as comments.
- `check --diff <rev>` so CI on a repo full of legacy prose judges only the lines a PR changed.
- Split the ruleset into `rules.d/*.toml` if one file gets unwieldy under a big phrase list.
- Judge eval harness: a synthetic corpus with expected findings, run against real Haiku on demand (never in CI) to catch prompt regressions.
