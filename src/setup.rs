//! `init`, `install` and `uninstall`: put a ruleset where the hook finds it and wire the hook into
//! Claude Code's settings without disturbing anything else in them.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::SystemTime;

use regex::Regex;
use serde_json::{Map, Value, json};

use crate::env::Env;
use crate::ruleset;

const MATCHER: &str = "Write|Edit|MultiEdit";
/// Room for judge calls (each capped at `judges.timeout_secs`, 45s by default) before Claude Code
/// gives up on the hook.
const TIMEOUT_SECS: u64 = 120;

static OURS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"voice-enforcer['"]?\s+hook$"#).expect("valid command regex"));

pub(crate) const EXAMPLE_RULESET: &str = include_str!("../examples/ruleset/ruleset.toml");

pub(crate) fn init(
    ruleset_flag: Option<&Path>,
    force: bool,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    env: &Env,
) -> u8 {
    let path = ruleset::locate(ruleset_flag, env);
    if path.exists() && !force {
        let _ = writeln!(
            stderr,
            "voice-enforcer: {} already exists; pass --force to overwrite it",
            path.display()
        );
        return 2;
    }
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, EXAMPLE_RULESET));
    match written {
        Ok(()) => {
            let _ = writeln!(stdout, "wrote the example ruleset to {}", path.display());
            0
        }
        Err(e) => {
            let _ = writeln!(
                stderr,
                "voice-enforcer: can't write {}: {e}",
                path.display()
            );
            2
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Install,
    Uninstall,
}

pub(crate) fn settings(
    action: Action,
    settings_flag: Option<&Path>,
    dry_run: bool,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    env: &Env,
) -> u8 {
    let path = settings_flag.map_or_else(
        || env.home.join(".claude/settings.json"),
        |p| env.cwd.join(p),
    );
    match edit_settings(action, &path, dry_run, env) {
        Ok(message) => {
            let _ = writeln!(stdout, "{message}");
            0
        }
        Err(message) => {
            let _ = writeln!(stderr, "voice-enforcer: {message}");
            2
        }
    }
}

fn edit_settings(action: Action, path: &Path, dry_run: bool, env: &Env) -> Result<String, String> {
    let shown = path.display();
    let original = match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("can't read {shown}: {e}")),
    };
    let mut root: Value = match &original {
        Some(text) => serde_json::from_str(text)
            .map_err(|e| format!("{shown} isn't valid JSON, leaving it alone: {e}"))?,
        None => json!({}),
    };
    let before = root.clone();
    let obj = root
        .as_object_mut()
        .ok_or_else(|| format!("{shown} isn't a JSON object"))?;

    match action {
        Action::Install => {
            let pre = pre_tool_use(obj).map_err(|what| format!("{shown}: {what}"))?;
            remove_ours(pre);
            pre.push(json!({
                "matcher": MATCHER,
                "hooks": [{"type": "command", "command": command(env), "timeout": TIMEOUT_SECS}],
            }));
        }
        Action::Uninstall => {
            if let Some(Value::Object(hooks)) = obj.get_mut("hooks") {
                if let Some(Value::Array(pre)) = hooks.get_mut("PreToolUse") {
                    remove_ours(pre);
                    if pre.is_empty() {
                        hooks.remove("PreToolUse");
                    }
                }
                if hooks.is_empty() {
                    obj.remove("hooks");
                }
            }
        }
    }

    let rendered =
        serde_json::to_string_pretty(&root).expect("a JSON value always serializes") + "\n";
    if dry_run {
        return Ok(rendered.trim_end().to_owned());
    }
    if root == before && original.is_some() {
        return Ok(match action {
            Action::Install => format!("the voice-enforcer hook is already installed in {shown}"),
            Action::Uninstall => format!("no voice-enforcer hook in {shown}, nothing changed"),
        });
    }
    if original.is_none() && action == Action::Uninstall {
        return Ok(format!("{shown} doesn't exist, nothing changed"));
    }

    let backup = match &original {
        Some(text) => {
            Some(backup(path, text, env.now).map_err(|e| format!("can't back up {shown}: {e}"))?)
        }
        None => None,
    };
    write_atomic(path, &rendered).map_err(|e| format!("can't write {shown}: {e}"))?;
    let verb = match action {
        Action::Install => "installed the voice-enforcer hook in",
        Action::Uninstall => "removed the voice-enforcer hook from",
    };
    Ok(match backup {
        Some(b) => format!("{verb} {shown} (backup: {})", b.display()),
        None => format!("{verb} {shown}"),
    })
}

/// `hooks.PreToolUse`, created when missing.
fn pre_tool_use(obj: &mut Map<String, Value>) -> Result<&mut Vec<Value>, &'static str> {
    let hooks = obj
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("`hooks` isn't an object")?;
    hooks
        .entry("PreToolUse")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("`hooks.PreToolUse` isn't an array")
}

/// Drops our handlers from every group, and any group left with no handlers because of it.
fn remove_ours(groups: &mut Vec<Value>) {
    groups.retain_mut(|group| {
        let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            return true;
        };
        let before = handlers.len();
        handlers.retain(|h| {
            !h.get("command")
                .and_then(Value::as_str)
                .is_some_and(|c| OURS.is_match(c.trim()))
        });
        !(handlers.is_empty() && before > 0)
    });
}

fn command(env: &Env) -> String {
    format!("{} hook", exe_for_shell(env))
}

/// The running binary's canonical path, quoted for a shell command line.
pub(crate) fn exe_for_shell(env: &Env) -> String {
    let exe = std::fs::canonicalize(&env.exe).unwrap_or_else(|_| env.exe.clone());
    shell_quote(&exe.display().to_string())
}

/// `word` as one shell word: unchanged when it's plain, single-quoted otherwise (single quotes
/// stop every expansion; an embedded quote becomes '\'').
pub(crate) fn shell_quote(word: &str) -> String {
    if word
        .chars()
        .any(|c| c.is_whitespace() || "'\"$`\\!*?[]{}()<>|&;#~".contains(c))
    {
        format!("'{}'", word.replace('\'', "'\\''"))
    } else {
        word.to_owned()
    }
}

fn backup(path: &Path, text: &str, now: SystemTime) -> std::io::Result<PathBuf> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let stamp = crate::clock::utc_stamp(now);
    let mut n = 0;
    loop {
        let suffix = if n == 0 {
            String::new()
        } else {
            format!("-{n}")
        };
        n += 1;
        let candidate = path.with_file_name(format!("{name}.bak-{stamp}{suffix}"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                file.write_all(text.as_bytes())?;
                return Ok(candidate);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
}

fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!(".{name}.tmp-{}", std::process::id()));
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// `YYYYMMDDHHMMSS` in UTC.
#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::SystemTime;

    use super::*;
    use crate::test_support::run_with;

    fn env(root: &Path) -> Env {
        Env {
            home: root.join("home"),
            cwd: root.to_path_buf(),
            xdg_config_home: Some(root.join("config")),
            xdg_state_home: None,
            xdg_cache_home: None,
            ruleset_dir: None,
            now: SystemTime::now(),
            exe: PathBuf::from("/bin/voice-enforcer"),
            judge_child: false,
        }
    }

    #[test]
    fn example_ruleset_is_valid_and_covers_every_kind() {
        let rs = ruleset::parse(EXAMPLE_RULESET, Path::new("example"), Path::new("/h")).unwrap();
        let kinds: std::collections::HashSet<_> = rs
            .rules
            .iter()
            .map(|r| std::mem::discriminant(&r.kind))
            .collect();
        assert_eq!(kinds.len(), 5);
        assert_eq!(rs.judges.len(), 2);
        assert!(
            rs.judges
                .iter()
                .all(|j| j.severity == crate::types::Severity::Warn)
        );
    }

    #[test]
    fn first_run_writes_a_valid_ruleset() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path());
        let out = run_with(&["init"], "", &env);
        assert_eq!(out.code, 0, "{}", out.stderr);
        let path = dir.path().join("config/voice-enforcer/ruleset.toml");
        assert!(out.stdout.contains(&path.display().to_string()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), EXAMPLE_RULESET);
        assert_eq!(run_with(&["validate"], "", &env).code, 0);
    }

    #[test]
    fn existing_ruleset_is_protected_unless_forced() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path());
        let path = dir.path().join("mine/ruleset.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "version = 1\n").unwrap();

        let out = run_with(&["init", "--ruleset", "mine"], "", &env);
        assert_eq!(out.code, 2);
        assert!(out.stderr.contains("--force"), "{}", out.stderr);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "version = 1\n");

        let out = run_with(&["init", "--ruleset", "mine", "--force"], "", &env);
        assert_eq!(out.code, 0);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), EXAMPLE_RULESET);
    }

    #[test]
    fn unwritable_location_exits_two() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path());
        std::fs::write(dir.path().join("blocker"), "a file, not a dir").unwrap();
        let out = run_with(&["init", "--ruleset", "blocker/sub"], "", &env);
        assert_eq!(out.code, 2);
        assert!(out.stderr.contains("can't write"), "{}", out.stderr);
    }
}

#[cfg(test)]
mod settings_tests;
