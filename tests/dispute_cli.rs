//! The dispute loop through the real binary: deny, dispute by id, record-only.

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

fn bin(dir: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("voice-enforcer").unwrap();
    cmd.env_clear()
        .env("HOME", dir.join("home"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("VOICE_ENFORCER_RULESET", dir.join("rules"));
    cmd
}

fn deny_reason(dir: &std::path::Path) -> String {
    let payload = json!({
        "session_id": "e2e",
        "cwd": dir,
        "tool_name": "Write",
        "tool_input": {"file_path": dir.join("README.md"), "content": "We leverage it.\n"},
    });
    let out = bin(dir)
        .arg("hook")
        .write_stdin(payload.to_string())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "deny");
    json["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn first_id(reason: &str) -> String {
    regex::Regex::new(r"\(id ([0-9a-f]{8})\)")
        .unwrap()
        .captures(reason)
        .expect("an id in the reason")[1]
        .to_owned()
}

#[test]
fn deny_then_dispute_then_still_denied() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("rules")).unwrap();
    std::fs::write(dir.path().join("rules/ruleset.toml"), RULES).unwrap();

    let reason = deny_reason(dir.path());
    assert!(reason.contains("dispute <id> --reason"), "{reason}");
    let id = first_id(&reason);

    bin(dir.path())
        .args(["dispute", &id, "--reason", "quoting the phrase on purpose"])
        .assert()
        .success()
        .stdout(predicates::str::contains("recorded the dispute in"));
    let disputes: Vec<_> = std::fs::read_dir(dir.path().join("rules/disputes/no-leverage"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(disputes.len(), 1);
    let text = std::fs::read_to_string(disputes[0].path()).unwrap();
    assert!(
        text.contains("reason = \"quoting the phrase on purpose\""),
        "{text}"
    );

    let again = deny_reason(dir.path());
    assert_eq!(first_id(&again), id, "record-only: same finding, same id");

    bin(dir.path())
        .args(["dispute", "deadbeef", "--reason", "wrong"])
        .assert()
        .code(2);
}
