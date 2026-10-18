# voice-enforcer

A Claude Code hook that holds everything Claude writes to YOUR voice, checked at the moment of the write instead of trusted to a style guide sitting in the context window. Style guides in context work right up until a long session pushes them out, then the drift starts quietly (and nobody notices until a README reads like a press release). A hook runs on every Write, Edit and MultiEdit no matter what the model remembers.

It ships with no voice of its own. You bring a ruleset (a TOML file that lives wherever you keep private things), the binary enforces it.

## What it does today

Every write gets its prose pulled out (markdown and plain text, plus the comments in Rust, Swift, TypeScript, JavaScript, Python and shell), then deterministic rules run against the lines that changed:

- banned phrases, with exceptions for the contexts where a phrase is fine
- raw regex for anything a phrase list can't express
- the Oxford comma (flagged, for those of us who hate it)
- comment length, budgeted separately for line, item and module comments
- file links: a file you name in a doc has to be a link, so a reader on GitHub can click through

Those rules catch the MECHANICAL tells. The judgment calls (a recap closer, a comment that just narrates the code under it) go to Haiku judges you define in the same ruleset, each one a rubric plus a scope: only the changed prose, or the whole file with the changes marked. Judges run only when nothing deterministic blocks and the write actually changed prose they read, all in parallel, so a code-only edit costs nothing and a judged write costs about one Haiku round trip (around 3s). They run through `claude -p` on your existing Claude login, no API key needed.

A finding either blocks the write (Claude gets the reason and rewrites before anything lands on disk) or rides along as a warning. Which one is your call, per rule and per judge.

To be clear about the limits: judges are a model, so they can miss things and they can misfire. A quote a judge returns has to actually be in the text or it gets dropped, and a judge that times out or errors becomes a note, never a block, but a judge can still flag something you'd keep (the example ones ship as warnings for that reason; marking a judge wrong is next on the [backlog](openspec/backlog.md)). It also governs Claude's writes only, nothing hooks what you type by hand.

## Install

From a checkout:

```sh
cargo install --path .
voice-enforcer init        # writes the example ruleset to ~/.config/voice-enforcer/
voice-enforcer validate
voice-enforcer install     # adds the hook to ~/.claude/settings.json (backup first)
```

Then replace the example rules with your own. If your ruleset lives in a private repo, symlink `~/.config/voice-enforcer` to it (or point `VOICE_ENFORCER_RULESET` at it) so the rules stay versioned there and out of anything you share. `voice-enforcer uninstall` takes the hook back out, touching nothing else in your settings.

`voice-enforcer check <paths>` runs the same rules over whole files, for CI or a one-off sweep (exit 1 when anything blocks). Add `--judges` to run the judges too; it's off by default since CI usually has no Claude login.

Upgrading from a version without judges? Re-run `voice-enforcer install`, it raises the hook timeout to 120s so judge calls fit (and replaces the old entry in place).

## Where to go next

- [docs/design.md](docs/design.md) owns the details: the hook contract, how prose gets extracted, every rule kind, how judges get called and checked and the measured latency.
- [The example ruleset](examples/ruleset/ruleset.toml) has one of each rule kind and two judges, commented with why each exists.
- [openspec/](openspec/) holds the plan; [openspec/backlog.md](openspec/backlog.md) is what's next.

## License

MIT OR Apache-2.0, your choice ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).
