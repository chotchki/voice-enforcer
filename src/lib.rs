//! voice-enforcer: holds Claude Code's written output to a voice ruleset at write time.

mod check;
mod clock;
mod disputes;
mod env;
mod extract;
mod hook;
mod judges;
mod pipeline;
mod rules;
mod ruleset;
mod sentence;
mod setup;
mod types;
mod walk;

pub use env::Env;

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "voice-enforcer", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Answer one Claude Code PreToolUse payload on stdin (what `install` wires up).
    Hook,
    /// Check files or directories whole, for CI or by hand.
    Check {
        /// Files or directories; directories skip git-ignored and excluded paths.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Ruleset directory, overriding `VOICE_ENFORCER_RULESET` and the config dir.
        #[arg(long)]
        ruleset: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = check::Format::Text)]
        format: check::Format,
        /// Also run the ruleset's LLM judges (off by default; needs an authenticated `claude`).
        #[arg(long)]
        judges: bool,
    },
    /// Write the example ruleset to the ruleset directory.
    Init {
        /// Ruleset directory, overriding `VOICE_ENFORCER_RULESET` and the config dir.
        #[arg(long)]
        ruleset: Option<PathBuf>,
        /// Overwrite an existing ruleset.
        #[arg(long)]
        force: bool,
    },
    /// Add the PreToolUse hook to Claude Code's settings (backing the file up first).
    Install {
        /// Settings file to edit instead of `~/.claude/settings.json`.
        #[arg(long)]
        settings: Option<PathBuf>,
        /// Print the resulting settings without writing anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove only the hook entries `install` added (backing the file up first).
    Uninstall {
        /// Settings file to edit instead of `~/.claude/settings.json`.
        #[arg(long)]
        settings: Option<PathBuf>,
    },
    /// Record that a finding from the hook is wrong, for the ruleset owner. Changes nothing else.
    Dispute {
        /// The finding's id, as shown in the hook's output.
        id: String,
        /// Why the finding is wrong.
        #[arg(long)]
        reason: String,
        /// Ruleset directory, overriding `VOICE_ENFORCER_RULESET` and the config dir.
        #[arg(long)]
        ruleset: Option<PathBuf>,
    },
    /// Load the ruleset and report whether it's valid.
    Validate {
        /// Ruleset directory, overriding `VOICE_ENFORCER_RULESET` and the config dir.
        #[arg(long)]
        ruleset: Option<PathBuf>,
    },
}

/// Runs one invocation and returns the process exit code.
pub fn run<I, T>(
    args: I,
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    env: &Env,
) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err) => return report_clap(&err, stdout, stderr),
    };
    match cli.command {
        Command::Hook => hook::run(stdin, stdout, stderr, env),
        Command::Check {
            paths,
            ruleset,
            format,
            judges,
        } => check::check(
            &paths,
            ruleset.as_deref(),
            format,
            judges,
            stdout,
            stderr,
            env,
        ),
        Command::Validate { ruleset } => check::validate(ruleset.as_deref(), stdout, stderr, env),
        Command::Install { settings, dry_run } => setup::settings(
            setup::Action::Install,
            settings.as_deref(),
            dry_run,
            stdout,
            stderr,
            env,
        ),
        Command::Uninstall { settings } => setup::settings(
            setup::Action::Uninstall,
            settings.as_deref(),
            false,
            stdout,
            stderr,
            env,
        ),
        Command::Dispute {
            id,
            reason,
            ruleset,
        } => disputes::dispute(&id, &reason, ruleset.as_deref(), stdout, stderr, env),
        Command::Init { ruleset, force } => {
            setup::init(ruleset.as_deref(), force, stdout, stderr, env)
        }
    }
}

fn report_clap(err: &clap::Error, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    use clap::error::ErrorKind;
    let rendered = err.render().to_string();
    if matches!(
        err.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    ) {
        let _ = write!(stdout, "{rendered}");
        0
    } else {
        let _ = write!(stderr, "{rendered}");
        2
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub struct Output {
        pub code: u8,
        pub stdout: String,
        pub stderr: String,
    }

    pub fn run_with(args: &[&str], stdin: &str, env: &Env) -> Output {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let argv = std::iter::once("voice-enforcer").chain(args.iter().copied());
        let code = run(argv, &mut stdin.as_bytes(), &mut out, &mut err, env);
        Output {
            code,
            stdout: String::from_utf8(out).unwrap(),
            stderr: String::from_utf8(err).unwrap(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::run_with;
    use super::*;

    #[test]
    fn version_goes_to_stdout() {
        let out = run_with(&["--version"], "", &Env::from_process());
        assert_eq!(out.code, 0);
        assert_eq!(
            out.stdout.trim(),
            format!("voice-enforcer {}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn no_subcommand_is_a_usage_error() {
        let out = run_with(&[], "", &Env::from_process());
        assert_eq!(out.code, 2);
        assert!(out.stderr.contains("Usage"));
    }
}
