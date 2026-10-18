//! `check` and `validate` through the real binary: exit codes are the CI contract.

use assert_cmd::Command;

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
    cmd.current_dir(dir)
        .env_clear()
        .env("HOME", dir.join("home"))
        .env("VOICE_ENFORCER_RULESET", dir.join("rules"));
    cmd
}

#[test]
fn exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("rules")).unwrap();
    std::fs::write(dir.path().join("rules/ruleset.toml"), RULES).unwrap();
    std::fs::write(dir.path().join("good.md"), "Plain.\n").unwrap();
    std::fs::write(dir.path().join("bad.md"), "We leverage it.\n").unwrap();

    bin(dir.path())
        .args(["check", "good.md"])
        .assert()
        .code(0)
        .stdout("");
    bin(dir.path())
        .args(["check", "bad.md"])
        .assert()
        .code(1)
        .stdout("bad.md:1: block no-leverage: Marketing verb (\"leverage\")\n");
    bin(dir.path()).args(["check", "nope.md"]).assert().code(2);
    bin(dir.path())
        .arg("validate")
        .assert()
        .code(0)
        .stdout(predicates::str::contains("valid, 1 rule(s)"));
}
