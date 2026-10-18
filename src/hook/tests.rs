use std::path::PathBuf;
use std::time::SystemTime;

use serde_json::Value;
use tempfile::TempDir;

use super::*;

const RULES: &str = r#"version = 1

[loop_guard]
max_consecutive_blocks = 3

[[rule]]
id = "no-leverage"
kind = "phrase"
severity = "block"
message = "Marketing verb"
suggestion = "say what it does"
phrases = ["leverage"]

[[rule]]
id = "no-very"
kind = "phrase"
severity = "warn"
message = "Intensifier"
phrases = ["very"]
"#;

struct Fixture {
    _dir: TempDir,
    env: Env,
    repo: PathBuf,
}

impl Fixture {
    fn new(rules: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let repo = root.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let rules_dir = root.join("rules");
        std::fs::create_dir_all(&rules_dir).unwrap();
        if let Some(rules) = rules {
            std::fs::write(rules_dir.join("ruleset.toml"), rules).unwrap();
        }
        let env = Env {
            home: root.join("home"),
            cwd: root.join("elsewhere"),
            xdg_config_home: None,
            xdg_state_home: Some(root.join("state")),
            ruleset_dir: Some(rules_dir),
            now: SystemTime::now(),
            exe: PathBuf::from("/bin/voice-enforcer"),
        };
        Self {
            _dir: dir,
            env,
            repo,
        }
    }

    fn file(&self, name: &str, content: &str) -> PathBuf {
        let path = self.repo.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn payload(&self, session: &str, tool: &str, input: &Value) -> String {
        json!({
            "session_id": session,
            "transcript_path": "/tmp/t.jsonl",
            "cwd": self.repo,
            "hook_event_name": "PreToolUse",
            "permission_mode": "default",
            "tool_name": tool,
            "tool_input": input,
            "tool_use_id": "toolu_01",
        })
        .to_string()
    }

    fn run(&self, payload: &str) -> Out {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(&mut payload.as_bytes(), &mut out, &mut err, &self.env);
        let stdout = String::from_utf8(out).unwrap();
        let json = (!stdout.is_empty()).then(|| serde_json::from_str(&stdout).unwrap());
        if let Some(json) = &json {
            assert_never_grants(json);
        }
        Out {
            code,
            json,
            stderr: String::from_utf8(err).unwrap(),
        }
    }

    fn write(&self, name: &str, content: &str) -> Out {
        self.run(&self.payload(
            "s1",
            "Write",
            &json!({"file_path": self.repo.join(name), "content": content}),
        ))
    }
}

struct Out {
    code: u8,
    json: Option<Value>,
    stderr: String,
}

impl Out {
    fn silent(&self) -> bool {
        self.code == 0 && self.json.is_none()
    }

    fn decision(&self) -> Option<&str> {
        self.json.as_ref()?["hookSpecificOutput"]["permissionDecision"].as_str()
    }

    fn reason(&self) -> &str {
        self.json.as_ref().unwrap()["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
    }

    fn context(&self) -> &str {
        self.json.as_ref().unwrap()["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
    }
}

fn assert_never_grants(json: &Value) {
    let output = &json["hookSpecificOutput"];
    assert_eq!(output["hookEventName"], "PreToolUse");
    let decision = output["permissionDecision"].as_str();
    assert!(
        matches!(decision, None | Some("deny")),
        "never allow or ask: {decision:?}"
    );
}

#[test]
fn unhandled_tool_passes_through() {
    let fx = Fixture::new(Some(RULES));
    let out = fx.run(&fx.payload("s1", "Bash", &json!({"command": "echo leverage"})));
    assert!(out.silent());
}

#[test]
fn blocking_write_is_denied_with_every_finding() {
    let fx = Fixture::new(Some(RULES));
    let out = fx.write("README.md", "# Title\n\nWe leverage a very fast cache.\n");
    assert_eq!(out.code, 0);
    assert_eq!(out.decision(), Some("deny"));
    insta::assert_snapshot!("deny_reason", out.reason());
    for needle in [
        "no-leverage",
        "[block]",
        "README.md:3",
        "\"leverage\"",
        "Marketing verb",
        "say what it does",
        "no-very",
    ] {
        assert!(
            out.reason().contains(needle),
            "{needle} missing from {}",
            out.reason()
        );
    }
}

#[test]
fn warnings_ride_along_without_a_decision() {
    let fx = Fixture::new(Some(RULES));
    let out = fx.write("README.md", "A very fast cache.\n");
    assert_eq!(out.decision(), None);
    insta::assert_snapshot!("warn_context", out.context());
}

#[test]
fn clean_write_is_silent() {
    let fx = Fixture::new(Some(RULES));
    assert!(fx.write("README.md", "A fast cache.\n").silent());
}

#[test]
fn edit_is_judged_against_the_edited_file() {
    let fx = Fixture::new(Some(RULES));
    let path = fx.file("README.md", "Intro.\n\nWe use the cache.\n");
    let out = fx.run(&fx.payload(
        "s1",
        "Edit",
        &json!({"file_path": path, "old_string": "use the cache", "new_string": "leverage the cache"}),
    ));
    assert_eq!(out.decision(), Some("deny"));
    assert!(out.reason().contains("README.md:3"), "{}", out.reason());
}

#[test]
fn ambiguous_edit_passes_through() {
    let fx = Fixture::new(Some(RULES));
    let path = fx.file("README.md", "a a\n");
    let out = fx.run(&fx.payload(
        "s1",
        "Edit",
        &json!({"file_path": path, "old_string": "a", "new_string": "leverage"}),
    ));
    assert!(out.silent());
}

#[test]
fn multi_edit_applies_edits_in_order() {
    let fx = Fixture::new(Some(RULES));
    let path = fx.file("README.md", "We use it.\n");
    let out = fx.run(&fx.payload(
        "s1",
        "MultiEdit",
        &json!({"file_path": path, "edits": [
            {"old_string": "use", "new_string": "lev"},
            {"old_string": "lev", "new_string": "leverage", "replace_all": true},
        ]}),
    ));
    assert_eq!(out.decision(), Some("deny"));
}

#[test]
fn untouched_lines_are_not_judged() {
    let fx = Fixture::new(Some(RULES));
    let path = fx.file("README.md", "Line one.\n\nWe leverage it.\n\nLine five.\n");
    let edit = |old: &str, new: &str| {
        fx.run(&fx.payload(
            "s1",
            "Edit",
            &json!({"file_path": path, "old_string": old, "new_string": new}),
        ))
    };
    assert!(edit("Line five.", "Line five, edited.").silent());
    let out = edit("We leverage it.", "We leverage it daily.");
    assert_eq!(
        out.decision(),
        Some("deny"),
        "touching the line makes it count"
    );
}

#[test]
fn new_file_is_judged_whole() {
    let fx = Fixture::new(Some(RULES));
    let out = fx.write("NEW.md", "Fine.\n\nleverage\n\nleverage\n");
    assert_eq!(out.reason().matches("no-leverage").count(), 2);
}

#[test]
fn relative_file_path_resolves_against_cwd() {
    let fx = Fixture::new(Some(RULES));
    let out = fx.run(&fx.payload(
        "s1",
        "Write",
        &json!({"file_path": "docs.md", "content": "leverage\n"}),
    ));
    assert!(out.reason().contains(" docs.md:1"), "{}", out.reason());
}

#[test]
fn paths_outside_cwd_display_absolute() {
    let fx = Fixture::new(Some(RULES));
    let outside = fx.env.home.join("x.md");
    std::fs::create_dir_all(&fx.env.home).unwrap();
    let out = fx.run(&fx.payload(
        "s1",
        "Write",
        &json!({"file_path": outside, "content": "leverage\n"}),
    ));
    assert!(
        out.reason().contains(&format!("{}:1", outside.display())),
        "{}",
        out.reason()
    );
}

#[test]
fn missing_cwd_and_session_fall_back() {
    let fx = Fixture::new(Some(RULES));
    let payload = json!({
        "tool_name": "Write",
        "tool_input": {"file_path": fx.repo.join("a.md"), "content": "leverage\n"},
    });
    let out = fx.run(&payload.to_string());
    assert_eq!(out.decision(), Some("deny"));
    assert!(fx.env.state_dir().join("sessions/unknown").exists());
}

#[test]
fn excluded_and_unknown_files_pass() {
    let fx = Fixture::new(Some(RULES));
    std::fs::create_dir_all(fx.repo.join("node_modules")).unwrap();
    assert!(fx.write("node_modules/README.md", "leverage\n").silent());
    assert!(fx.write("data.csv", "leverage\n").silent());
}

#[test]
fn unreadable_target_passes_through() {
    let fx = Fixture::new(Some(RULES));
    std::fs::create_dir_all(fx.repo.join("dir.md")).unwrap();
    assert!(fx.write("dir.md", "leverage\n").silent());
}

#[test]
fn garbage_stdin_is_a_non_blocking_error() {
    let fx = Fixture::new(Some(RULES));
    let out = fx.run("not json");
    assert_eq!(out.code, 1);
    assert!(out.json.is_none());
    assert!(
        out.stderr.contains("not a PreToolUse payload"),
        "{}",
        out.stderr
    );
}

#[test]
fn bad_tool_input_is_a_non_blocking_error() {
    let fx = Fixture::new(Some(RULES));
    let out = fx.run(&fx.payload("s1", "Edit", &json!({"file_path": "a.md"})));
    assert_eq!(out.code, 1);
    assert!(
        out.stderr.contains("unexpected Edit tool_input"),
        "{}",
        out.stderr
    );
}

#[test]
fn unreadable_stdin_is_a_non_blocking_error() {
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("boom"))
        }
    }
    let fx = Fixture::new(Some(RULES));
    let mut err = Vec::new();
    let code = run(&mut Broken, &mut Vec::new(), &mut err, &fx.env);
    assert_eq!(code, 1);
    assert!(
        String::from_utf8(err)
            .unwrap()
            .contains("can't read the hook payload")
    );
}

#[test]
fn broken_ruleset_is_surfaced_not_enforced() {
    let fx = Fixture::new(Some(
        "version = 1\n[[rule]]\nid = \"bad\"\nkind = \"regex\"\nseverity = \"block\"\nmessage = \"m\"\npattern = \"(\"\n",
    ));
    let out = fx.write("README.md", "leverage\n");
    assert_eq!(out.code, 0);
    assert_eq!(out.decision(), None);
    assert!(
        out.context().contains("failed to load"),
        "{}",
        out.context()
    );
    assert!(out.context().contains("rule `bad`"), "{}", out.context());
}

#[test]
fn no_ruleset_is_silent() {
    let fx = Fixture::new(None);
    assert!(fx.write("README.md", "leverage\n").silent());
}

#[test]
fn loop_guard_trips_after_three_denies() {
    let fx = Fixture::new(Some(RULES));
    for _ in 0..3 {
        assert_eq!(fx.write("README.md", "leverage\n").decision(), Some("deny"));
    }
    let fourth = fx.write("README.md", "leverage\n");
    assert_eq!(fourth.decision(), None);
    insta::assert_snapshot!("tripped_context", fourth.context());
}

#[test]
fn loop_guard_resets_on_a_clean_write() {
    let fx = Fixture::new(Some(RULES));
    fx.write("README.md", "leverage\n");
    fx.write("README.md", "leverage\n");
    assert!(fx.write("README.md", "clean\n").silent());
    for _ in 0..3 {
        assert_eq!(fx.write("README.md", "leverage\n").decision(), Some("deny"));
    }
}

#[test]
fn loop_guard_sessions_are_independent() {
    let fx = Fixture::new(Some(RULES));
    for _ in 0..4 {
        fx.write("README.md", "leverage\n");
    }
    let other = fx.run(&fx.payload(
        "s2",
        "Write",
        &json!({"file_path": fx.repo.join("README.md"), "content": "leverage\n"}),
    ));
    assert_eq!(other.decision(), Some("deny"));
}

#[test]
fn helpers() {
    assert_eq!(changed_lines(None, "a\nb\n"), None);
    let changed = changed_lines(Some("a\nb\nc\n"), "a\nB\nc\nd\n").unwrap();
    let mut changed: Vec<_> = changed.into_iter().collect();
    changed.sort_unstable();
    assert_eq!(changed, vec![2, 4]);
    assert_eq!(
        display_path(Path::new("/r/docs/a.md"), Path::new("/r")),
        "docs/a.md"
    );
}
