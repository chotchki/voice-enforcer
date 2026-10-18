//! Runs the real binary the way Claude Code does: hook JSON on stdin, decision JSON on stdout.

use assert_cmd::Command;
use serde_json::{Value, json};

const RULES: &str = r#"version = 1
[[rule]]
id = "no-leverage"
kind = "phrase"
severity = "block"
message = "Marketing verb"
phrases = ["leverage"]
"#;

fn hook(dir: &std::path::Path, payload: &Value) -> assert_cmd::assert::Assert {
    Command::cargo_bin("voice-enforcer")
        .unwrap()
        .arg("hook")
        .env_clear()
        .env("HOME", dir.join("home"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("VOICE_ENFORCER_RULESET", dir.join("rules"))
        .write_stdin(payload.to_string())
        .assert()
}

fn write_payload(dir: &std::path::Path, content: &str) -> Value {
    json!({
        "session_id": "e2e",
        "cwd": dir,
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "tool_input": {"file_path": dir.join("README.md"), "content": content},
    })
}

#[test]
fn deny_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("rules")).unwrap();
    std::fs::write(dir.path().join("rules/ruleset.toml"), RULES).unwrap();

    let out = hook(dir.path(), &write_payload(dir.path(), "We leverage it.\n"))
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(
        json["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .contains("no-leverage at README.md:1")
    );

    hook(dir.path(), &write_payload(dir.path(), "We use it.\n"))
        .success()
        .stdout("");
}

#[test]
fn no_ruleset_and_garbage() {
    let dir = tempfile::tempdir().unwrap();
    hook(dir.path(), &write_payload(dir.path(), "We leverage it.\n"))
        .success()
        .stdout("");
    Command::cargo_bin("voice-enforcer")
        .unwrap()
        .arg("hook")
        .write_stdin("{nope")
        .assert()
        .code(1);
}
