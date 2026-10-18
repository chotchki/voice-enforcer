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

A finding either blocks the write (Claude gets the reason and rewrites before anything lands on disk) or rides along as a warning. Which one is your call, per rule.

To be clear about the limits: these rules catch the MECHANICAL tells. Judgment calls (a recap closer, a triad in disguise, a comment that just restates the code) need a model, and Haiku judges for those are planned but not built yet ([backlog](openspec/backlog.md)). It also governs Claude's writes only, nothing hooks what you type by hand.

## Install

From a checkout:

```sh
cargo install --path .
voice-enforcer init        # writes the example ruleset to ~/.config/voice-enforcer/
voice-enforcer validate
voice-enforcer install     # adds the hook to ~/.claude/settings.json (backup first)
```

Then replace the example rules with your own. If your ruleset lives in a private repo, symlink `~/.config/voice-enforcer` to it (or point `VOICE_ENFORCER_RULESET` at it) so the rules stay versioned there and out of anything you share. `voice-enforcer uninstall` takes the hook back out, touching nothing else in your settings.

`voice-enforcer check <paths>` runs the same rules over whole files, for CI or a one-off sweep (exit 1 when anything blocks).

## Where to go next

- [docs/design.md](docs/design.md) owns the details: the hook contract, how prose gets extracted, every rule kind with its config and the measured latency.
- [The example ruleset](examples/ruleset/ruleset.toml) has one of each rule kind, commented with why it exists.
- [openspec/](openspec/) holds the plan; [openspec/backlog.md](openspec/backlog.md) is what's next.

## License

MIT OR Apache-2.0, your choice ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).
