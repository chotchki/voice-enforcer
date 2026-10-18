use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::test_support::{Output, run_with};

/// 2026-10-17 19:30:05 UTC
const NOW: u64 = 1_792_265_405;

struct Fixture {
    dir: TempDir,
    env: Env,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("voice-enforcer"), "").unwrap();
        let env = Env {
            home: dir.path().join("home"),
            cwd: dir.path().to_path_buf(),
            xdg_config_home: None,
            xdg_state_home: None,
            ruleset_dir: None,
            now: SystemTime::UNIX_EPOCH + Duration::from_secs(NOW),
            exe: bin.join("voice-enforcer"),
        };
        Self { dir, env }
    }

    fn settings_path(&self) -> PathBuf {
        self.env.home.join(".claude/settings.json")
    }

    fn seed(&self, value: &Value) {
        let path = self.settings_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string_pretty(value).unwrap()).unwrap();
    }

    /// Puts a settings file in a read-only directory by briefly lifting the restriction.
    #[cfg(unix)]
    fn seed_unchecked(&self) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.settings_path();
        let dir = path.parent().unwrap();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(&path, "{}").unwrap();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    }

    fn read(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.settings_path()).unwrap()).unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        run_with(args, "", &self.env)
    }

    fn our_command(&self) -> String {
        format!(
            "{} hook",
            std::fs::canonicalize(&self.env.exe).unwrap().display()
        )
    }

    fn backups(&self) -> Vec<PathBuf> {
        let mut found: Vec<_> = std::fs::read_dir(self.settings_path().parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.to_string_lossy().contains(".bak-"))
            .collect();
        found.sort();
        found
    }
}

fn existing_settings() -> Value {
    json!({
        "permissions": {"allow": ["Bash(sed:*)"]},
        "hooks": {
            "Notification": [
                {"matcher": "", "hooks": [{"type": "command", "command": "afplay ping.aiff"}]}
            ],
            "PreToolUse": [
                {"matcher": "Bash", "hooks": [{"type": "command", "command": "guard-bash"}]}
            ]
        },
        "model": "opus"
    })
}

fn ours_count(settings: &Value) -> usize {
    settings["hooks"]["PreToolUse"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["hooks"].as_array().unwrap())
        .filter(|h| OURS.is_match(h["command"].as_str().unwrap()))
        .count()
}

#[test]
fn install_keeps_existing_settings_and_backs_up() {
    let fx = Fixture::new();
    fx.seed(&existing_settings());
    let original = std::fs::read_to_string(fx.settings_path()).unwrap();

    let out = fx.run(&["install"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("installed"), "{}", out.stdout);

    let got = fx.read();
    assert_eq!(got["permissions"], existing_settings()["permissions"]);
    assert_eq!(got["model"], "opus");
    assert_eq!(
        got["hooks"]["Notification"],
        existing_settings()["hooks"]["Notification"]
    );
    let pre = got["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre[0], existing_settings()["hooks"]["PreToolUse"][0]);
    assert_eq!(
        pre[1],
        json!({
            "matcher": "Write|Edit|MultiEdit",
            "hooks": [{"type": "command", "command": fx.our_command(), "timeout": 30}]
        })
    );

    let backups = fx.backups();
    assert_eq!(backups.len(), 1);
    assert!(
        backups[0].ends_with("settings.json.bak-20261017193005"),
        "{backups:?}"
    );
    assert_eq!(std::fs::read_to_string(&backups[0]).unwrap(), original);
}

#[test]
fn install_keeps_key_order() {
    let fx = Fixture::new();
    fx.seed(&existing_settings());
    fx.run(&["install"]);
    let text = std::fs::read_to_string(fx.settings_path()).unwrap();
    let positions: Vec<_> = ["permissions", "hooks", "model"]
        .iter()
        .map(|k| text.find(&format!("\"{k}\"")).unwrap())
        .collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{text}");
}

#[test]
fn install_is_idempotent() {
    let fx = Fixture::new();
    fx.seed(&existing_settings());
    fx.run(&["install"]);
    let out = fx.run(&["install"]);
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("already installed"), "{}", out.stdout);
    assert_eq!(ours_count(&fx.read()), 1);
    assert_eq!(fx.backups().len(), 1, "an unchanged file isn't rewritten");
}

#[test]
fn install_replaces_a_stale_binary_path() {
    let fx = Fixture::new();
    fx.seed(&json!({"hooks": {"PreToolUse": [
        {"matcher": "Write|Edit|MultiEdit", "hooks": [
            {"type": "command", "command": "/old/place/voice-enforcer hook", "timeout": 30}
        ]}
    ]}}));
    fx.run(&["install"]);
    let got = fx.read();
    assert_eq!(ours_count(&got), 1);
    assert_eq!(
        got["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        fx.our_command()
    );
}

#[test]
fn install_creates_a_missing_file() {
    let fx = Fixture::new();
    let out = fx.run(&["install"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(ours_count(&fx.read()), 1);
    assert_eq!(fx.backups().len(), 0, "nothing to back up");
}

#[test]
fn dry_run_writes_nothing() {
    let fx = Fixture::new();
    let out = fx.run(&["install", "--dry-run"]);
    assert_eq!(out.code, 0);
    let printed: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(ours_count(&printed), 1);
    assert!(!fx.env.home.exists(), "no file or directory created");

    fx.seed(&existing_settings());
    let before = std::fs::read_to_string(fx.settings_path()).unwrap();
    fx.run(&["install", "--dry-run"]);
    assert_eq!(std::fs::read_to_string(fx.settings_path()).unwrap(), before);
    assert_eq!(fx.backups().len(), 0);
}

#[test]
fn settings_flag_targets_another_file() {
    let fx = Fixture::new();
    let out = fx.run(&["install", "--settings", "project/.claude/settings.json"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(fx.dir.path().join("project/.claude/settings.json").exists());
    assert!(!fx.settings_path().exists());
}

#[test]
fn malformed_settings_are_left_alone() {
    let fx = Fixture::new();
    for (content, needle) in [
        ("{not json", "isn't valid JSON"),
        ("[1, 2]", "isn't a JSON object"),
        ("{\"hooks\": []}", "`hooks` isn't an object"),
        (
            "{\"hooks\": {\"PreToolUse\": {}}}",
            "`hooks.PreToolUse` isn't an array",
        ),
    ] {
        let path = fx.settings_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        let out = fx.run(&["install"]);
        assert_eq!(out.code, 2, "{content}");
        assert!(out.stderr.contains(needle), "{content}: {}", out.stderr);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
    }
}

#[test]
fn unreadable_settings_exit_two() {
    let fx = Fixture::new();
    std::fs::create_dir_all(fx.settings_path()).unwrap();
    let out = fx.run(&["install"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("can't read"), "{}", out.stderr);
}

#[cfg(unix)]
fn read_only(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
}

#[cfg(unix)]
#[test]
fn unwritable_settings_exit_two() {
    let fx = Fixture::new();
    let dir = fx.settings_path().parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&dir).unwrap();
    read_only(&dir);
    let out = fx.run(&["install"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("can't write"), "{}", out.stderr);

    fx.seed_unchecked();
    let out = fx.run(&["install"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("can't back up"), "{}", out.stderr);
}

#[test]
fn uninstall_removes_only_ours() {
    let fx = Fixture::new();
    fx.seed(&existing_settings());
    fx.run(&["install"]);
    let out = fx.run(&["uninstall"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("removed"), "{}", out.stdout);
    assert_eq!(fx.read(), existing_settings());
    assert_eq!(fx.backups().len(), 2);
}

#[test]
fn uninstall_keeps_a_shared_group() {
    let fx = Fixture::new();
    let cmd = fx.our_command();
    fx.seed(&json!({"hooks": {"PreToolUse": [
        {"matcher": "Write", "hooks": [
            {"type": "command", "command": cmd},
            {"type": "command", "command": "other-tool"}
        ]},
        {"matcher": "Edit", "hooks": []},
        {"matcher": "Odd"}
    ]}}));
    fx.run(&["uninstall"]);
    assert_eq!(
        fx.read(),
        json!({"hooks": {"PreToolUse": [
            {"matcher": "Write", "hooks": [{"type": "command", "command": "other-tool"}]},
            {"matcher": "Edit", "hooks": []},
            {"matcher": "Odd"}
        ]}})
    );
}

#[test]
fn uninstall_cleans_up_what_it_emptied() {
    let fx = Fixture::new();
    fx.run(&["install"]);
    fx.run(&["uninstall"]);
    assert_eq!(fx.read(), json!({}));
}

#[test]
fn uninstall_with_nothing_to_remove() {
    let fx = Fixture::new();
    let out = fx.run(&["uninstall"]);
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("doesn't exist"), "{}", out.stdout);

    fx.seed(&existing_settings());
    let before = std::fs::read_to_string(fx.settings_path()).unwrap();
    let out = fx.run(&["uninstall"]);
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("nothing changed"), "{}", out.stdout);
    assert_eq!(std::fs::read_to_string(fx.settings_path()).unwrap(), before);
    assert_eq!(fx.backups().len(), 0);
}

#[test]
fn backups_never_overwrite_each_other() {
    let fx = Fixture::new();
    fx.seed(&json!({}));
    let path = fx.settings_path();
    let first = backup(&path, "one", fx.env.now).unwrap();
    let second = backup(&path, "two", fx.env.now).unwrap();
    assert_ne!(first, second);
    assert!(
        second.to_string_lossy().ends_with("-1"),
        "{}",
        second.display()
    );
    assert_eq!(std::fs::read_to_string(first).unwrap(), "one");
}

#[test]
fn backup_errors_surface() {
    let err = backup(
        Path::new("/nonexistent-dir/x/settings.json"),
        "x",
        SystemTime::UNIX_EPOCH,
    );
    assert!(err.is_err());
}

#[test]
fn quoted_command_for_awkward_paths() {
    let fx = Fixture::new();
    let mut env = fx.env.clone();
    env.exe = PathBuf::from("/nowhere/My Tools/it's/voice-enforcer");
    let cmd = command(&env);
    assert_eq!(cmd, r"'/nowhere/My Tools/it'\''s/voice-enforcer' hook");
    assert!(OURS.is_match(&cmd));
    env.exe = PathBuf::from("/nowhere/plain/voice-enforcer");
    assert_eq!(command(&env), "/nowhere/plain/voice-enforcer hook");
}

#[test]
fn utc_stamps() {
    let at = |s: u64| utc_stamp(SystemTime::UNIX_EPOCH + Duration::from_secs(s));
    assert_eq!(at(0), "19700101000000");
    assert_eq!(at(951_868_800), "20000301000000");
    assert_eq!(at(1_709_210_096), "20240229123456");
    assert_eq!(at(NOW), "20261017193005");
    assert_eq!(
        utc_stamp(SystemTime::UNIX_EPOCH - Duration::from_secs(5)),
        "19700101000000"
    );
}

#[test]
fn uninstall_leaves_other_hook_events_alone() {
    let fx = Fixture::new();
    let settings = json!({"hooks": {"Notification": [{"matcher": "", "hooks": []}]}});
    fx.seed(&settings);
    fx.run(&["uninstall"]);
    assert_eq!(fx.read(), settings);
}

#[test]
fn failed_rename_cleans_up_its_temp_file() {
    let fx = Fixture::new();
    let target = fx.dir.path().join("occupied");
    std::fs::create_dir_all(target.join("inside")).unwrap();
    assert!(write_atomic(&target, "x").is_err());
    let leftovers: Vec<_> = std::fs::read_dir(fx.dir.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}
