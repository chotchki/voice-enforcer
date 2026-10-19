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
            xdg_cache_home: Some(root.join("cache")),
            ruleset_dir: Some(rules_dir),
            now: SystemTime::now(),
            exe: PathBuf::from("/bin/voice-enforcer"),
            judge_child: false,
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

    /// Makes the repo a real git repo and commits everything in it.
    fn commit(&self) {
        std::fs::remove_dir_all(self.repo.join(".git")).unwrap();
        for args in [
            &["init", "-q"][..],
            &["add", "-A"],
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "-m",
                "base",
            ],
        ] {
            let ok = std::process::Command::new("git")
                .arg("-C")
                .arg(&self.repo)
                .args(args)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?}");
        }
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

/// Ids hash the temp dir's path, so snapshots see a placeholder.
fn redact(text: &str) -> String {
    regex::Regex::new(r"\(id [0-9a-f]{8}\)")
        .unwrap()
        .replace_all(text, "(id ########)")
        .into_owned()
}

fn ids(text: &str) -> Vec<String> {
    regex::Regex::new(r"\(id ([0-9a-f]{8})\)")
        .unwrap()
        .captures_iter(text)
        .map(|c| c[1].to_owned())
        .collect()
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
    insta::assert_snapshot!("deny_reason", redact(out.reason()));
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
fn two_oxford_commas_in_one_file_get_two_ids() {
    let rules = format!(
        "{RULES}\n[[rule]]\nid = \"oxford\"\nkind = \"oxford_comma\"\nseverity = \"warn\"\nmessage = \"Oxford comma\"\n"
    );
    let fx = Fixture::new(Some(&rules));
    let out = fx.write(
        "README.md",
        "Pick tea, coffee, or water.\n\nBring forks, plates, and cups.\n",
    );
    let got = ids(out.context());
    assert_eq!(got.len(), 2, "{}", out.context());
    assert_ne!(got[0], got[1]);
}

#[test]
fn every_finding_has_an_id_and_the_hint_closes_the_listing() {
    let fx = Fixture::new(Some(RULES));
    let out = fx.write("README.md", "We leverage a very fast cache.\n");
    assert_eq!(ids(out.reason()).len(), 2);
    let last = out.reason().lines().last().unwrap();
    assert!(
        last.starts_with(
            "Think a finding is wrong? Run `/bin/voice-enforcer dispute <id> --reason"
        ),
        "{last}"
    );
    assert!(
        last.ends_with(
            "it doesn't change this result. A wrong warning needs no rewrite: dispute it and keep the text."
        ),
        "a mixed listing holds a warning: {last}"
    );
}

#[test]
fn the_keep_the_text_line_rides_only_with_warnings() {
    let fx = Fixture::new(Some(RULES));
    let blocks = fx.write("README.md", "We leverage it.\n");
    let last = blocks.reason().lines().last().unwrap();
    assert!(last.ends_with("it doesn't change this result."), "{last}");
    let warns = fx.write("NOTES.md", "A very fast cache.\n");
    let last = warns.context().lines().last().unwrap();
    assert!(last.ends_with("dispute it and keep the text."), "{last}");
}

#[test]
fn ids_survive_a_retry_that_moves_the_text() {
    let fx = Fixture::new(Some(RULES));
    let first = fx.write("README.md", "We leverage it.\n");
    let second = fx.write("README.md", "Intro.\n\nWe leverage it.\n");
    assert!(
        second.reason().contains("README.md:3"),
        "{}",
        second.reason()
    );
    assert_eq!(ids(first.reason()), ids(second.reason()));
    let other_session = fx.run(&fx.payload(
        "s2",
        "Write",
        &json!({"file_path": fx.repo.join("README.md"), "content": "We leverage it.\n"}),
    ));
    assert_ne!(ids(first.reason()), ids(other_session.reason()));
}

#[test]
fn listed_findings_leave_samples_behind() {
    let fx = Fixture::new(Some(RULES));
    let out = fx.write("README.md", "We leverage a very fast cache.\n");
    for id in ids(out.reason()) {
        let sample = crate::disputes::load(&fx.env.state_dir(), &id).expect("a sample per finding");
        assert_eq!(sample.id, id);
        assert_eq!(sample.context, "We leverage a very fast cache.");
        assert_eq!(sample.file, fx.repo.join("README.md").display().to_string());
    }
}

#[test]
fn unwritable_state_leaves_the_decision_alone() {
    let fx = Fixture::new(Some(RULES));
    std::fs::write(fx.env.state_dir().parent().unwrap(), "a file, not a dir").unwrap();
    let out = fx.write("README.md", "We leverage it.\n");
    assert_eq!(out.decision(), Some("deny"));
}

#[test]
fn notes_only_listing_has_no_hint() {
    let fx = Fixture::new(Some(
        "version = 1\n[[rule]]\nid = \"bad\"\nkind = \"regex\"\nseverity = \"block\"\nmessage = \"m\"\npattern = \"(\"\n",
    ));
    let out = fx.write("README.md", "leverage\n");
    assert!(!out.context().contains("dispute"), "{}", out.context());
}

#[test]
fn warnings_ride_along_without_a_decision() {
    let fx = Fixture::new(Some(RULES));
    let out = fx.write("README.md", "A very fast cache.\n");
    assert_eq!(out.decision(), None);
    insta::assert_snapshot!("warn_context", redact(out.context()));
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
fn rewrapping_a_paragraph_changes_nothing() {
    let fx = Fixture::new(Some(RULES));
    fx.file(
        "README.md",
        "Intro.\n\nWe leverage the cache so every\nlookup stays fast.\n",
    );
    let out = fx.write(
        "README.md",
        "Intro.\n\nWe leverage the cache\nso every lookup stays fast.\n",
    );
    assert!(out.silent(), "only line breaks moved");
}

#[test]
fn reindenting_a_comment_changes_nothing() {
    let fx = Fixture::new(Some(RULES));
    fx.file("lib.rs", "fn f() {\n        // leverage the pool\n}\n");
    let out = fx.write("lib.rs", "fn f() {\n    // leverage the pool\n}\n");
    assert!(out.silent());
    let out = fx.write("lib.rs", "fn f() {\n    // leverage the pool, always\n}\n");
    assert_eq!(out.decision(), Some("deny"), "a changed word still counts");
}

#[test]
fn text_already_at_head_is_unchanged() {
    let fx = Fixture::new(Some(RULES));
    fx.file(
        "main.rs",
        "fn main() {\n        // We leverage the pool so every worker shares it.\n        run();\n}\n",
    );
    fx.commit();
    let moved =
        "pub fn boot() {\n    // We leverage the pool so every worker shares it.\n    run();\n}\n";
    assert!(fx.write("boot.rs", moved).silent(), "a move, not a write");
    let reworded = moved.replace("every worker", "each worker");
    assert_eq!(fx.write("boot.rs", &reworded).decision(), Some("deny"));
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
    insta::assert_snapshot!("tripped_context", redact(fourth.context()));
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
    assert_eq!(
        display_path(Path::new("/r/docs/a.md"), Path::new("/r")),
        "docs/a.md"
    );
}

mod judges {
    use super::*;
    use crate::judges::fake::FakeClaude;

    fn rules_with_judges(
        fake: &FakeClaude,
        severity: &str,
        scope: &str,
        registers: &str,
    ) -> String {
        format!(
            r#"{RULES}
[judges]
command = "{}"
timeout_secs = 10

[[judge]]
id = "recap"
severity = "{severity}"
scope = "{scope}"
registers = {registers}
rubric = "RECAP_RUBRIC"
"#,
            fake.command()
        )
    }

    fn recap_fake() -> FakeClaude {
        FakeClaude::answering(&json!([
            {"quote": "In conclusion, it is fast.", "explanation": "Restates the point.", "suggestion": "Cut it."}
        ]))
    }

    #[test]
    fn judge_child_exits_silently() {
        let mut fx = Fixture::new(Some(RULES));
        fx.env.judge_child = true;
        assert!(fx.run("not even json").silent());
    }

    #[test]
    fn deterministic_block_skips_the_judges() {
        let fake = recap_fake();
        let fx = Fixture::new(Some(&rules_with_judges(
            &fake,
            "warn",
            "diff",
            "[\"long_form\"]",
        )));
        let out = fx.write("README.md", "We leverage it. In conclusion, it is fast.\n");
        assert_eq!(out.decision(), Some("deny"));
        assert!(!out.reason().contains("recap"), "{}", out.reason());
        assert_eq!(fake.calls(), 0);
    }

    #[test]
    fn judge_warnings_join_deterministic_warnings() {
        let fake = recap_fake();
        let fx = Fixture::new(Some(&rules_with_judges(
            &fake,
            "warn",
            "diff",
            "[\"long_form\"]",
        )));
        let out = fx.write(
            "README.md",
            "A very fast cache. In conclusion, it is fast.\n",
        );
        assert_eq!(out.decision(), None);
        insta::assert_snapshot!("judge_warn_context", redact(out.context()));
        assert_eq!(fake.calls(), 1);
    }

    #[test]
    fn only_a_whole_judge_flags_a_paragraph_copied_from_head() {
        for (scope, flagged) in [("whole", true), ("diff", false)] {
            let fake = recap_fake();
            let fx = Fixture::new(Some(&rules_with_judges(
                &fake,
                "warn",
                scope,
                "[\"long_form\"]",
            )));
            fx.file("README.md", "Intro.\n\nIn conclusion, it is fast.\n");
            fx.commit();
            let out = fx.write("NOTES.md", "In conclusion, it is fast.\n");
            assert_eq!(!out.silent(), flagged, "{scope}");
        }
    }

    #[test]
    fn blocking_judge_denies() {
        let fake = recap_fake();
        let fx = Fixture::new(Some(&rules_with_judges(
            &fake,
            "block",
            "diff",
            "[\"long_form\"]",
        )));
        let out = fx.write("README.md", "Fast cache. In conclusion, it is fast.\n");
        assert_eq!(out.decision(), Some("deny"));
        assert!(
            redact(out.reason()).contains(
                "[block] recap at README.md:1 (id ########): \"In conclusion, it is fast.\""
            ),
            "{}",
            out.reason()
        );
    }

    #[test]
    fn blocking_judge_counts_toward_the_loop_guard() {
        let fake = recap_fake();
        let fx = Fixture::new(Some(&rules_with_judges(
            &fake,
            "block",
            "diff",
            "[\"long_form\"]",
        )));
        for _ in 0..3 {
            let out = fx.write("README.md", "Fast cache. In conclusion, it is fast.\n");
            assert_eq!(out.decision(), Some("deny"));
        }
        let fourth = fx.write("README.md", "Fast cache. In conclusion, it is fast.\n");
        assert_eq!(fourth.decision(), None);
        assert!(fourth.context().contains("loop guard tripped"));
        assert_eq!(fake.calls(), 1, "identical drafts come from the cache");
    }

    #[test]
    fn judge_failure_is_a_note_never_a_deny() {
        let fake = FakeClaude::new("garbage", 0, 0.0);
        let fx = Fixture::new(Some(&rules_with_judges(
            &fake,
            "block",
            "diff",
            "[\"long_form\"]",
        )));
        let out = fx.write("README.md", "Fast cache. In conclusion, it is fast.\n");
        assert_eq!(out.decision(), None);
        assert!(
            out.context()
                .contains("- note: judge `recap` returned a verdict that didn't parse"),
            "{}",
            out.context()
        );

        let out = fx.write("README.md", "A very fast cache.\n");
        assert!(
            out.context().contains("[warn] no-very"),
            "{}",
            out.context()
        );
        assert!(
            out.context().contains("- note: judge `recap`"),
            "{}",
            out.context()
        );
    }

    #[test]
    fn code_only_edit_makes_no_calls() {
        let fake = recap_fake();
        let fx = Fixture::new(Some(&rules_with_judges(
            &fake,
            "warn",
            "diff",
            "[\"long_form\", \"comment\"]",
        )));
        let path = fx.file(
            "lib.rs",
            "// Adds one.\nfn f(x: u8) -> u8 {\n    x + 1\n}\n",
        );
        let out = fx.run(&fx.payload(
            "s1",
            "Edit",
            &json!({"file_path": path, "old_string": "x + 1", "new_string": "x + 2"}),
        ));
        assert!(out.silent());
        assert_eq!(fake.calls(), 0);
    }

    #[test]
    fn register_filter_skips_the_judge() {
        let fake = recap_fake();
        let fx = Fixture::new(Some(&rules_with_judges(
            &fake,
            "warn",
            "diff",
            "[\"long_form\"]",
        )));
        assert!(
            fx.write("lib.rs", "// In conclusion, it is fast.\nfn f() {}\n")
                .silent()
        );
        assert_eq!(fake.calls(), 0);
    }

    #[test]
    fn whole_judge_quoting_untouched_text_is_dropped() {
        let fake = recap_fake();
        let fx = Fixture::new(Some(&rules_with_judges(
            &fake,
            "block",
            "whole",
            "[\"long_form\"]",
        )));
        let path = fx.file(
            "README.md",
            "In conclusion, it is fast.\n\nOld second paragraph.\n",
        );
        let out = fx.run(&fx.payload(
            "s1",
            "Edit",
            &json!({"file_path": path, "old_string": "Old second paragraph.", "new_string": "New second paragraph."}),
        ));
        assert!(
            out.silent(),
            "the quote sits on line 1, which didn't change"
        );
        assert_eq!(fake.calls(), 1);
        assert!(
            fake.stdin().contains("In conclusion, it is fast."),
            "a whole judge sees the old text"
        );
    }

    #[test]
    fn untouched_sentence_on_an_edited_one_line_paragraph_is_dropped() {
        // The first edit adds "is", which the quote holds too; the second only deletes a word.
        for (old, new) in [("stays", "is"), ("stays warm", "stays")] {
            let fake = recap_fake();
            let fx = Fixture::new(Some(&rules_with_judges(
                &fake,
                "block",
                "whole",
                "[\"long_form\"]",
            )));
            let path = fx.file(
                "README.md",
                "In conclusion, it is fast. The cache stays warm.\n",
            );
            let out = fx.run(&fx.payload(
                "s1",
                "Edit",
                &json!({"file_path": path, "old_string": old, "new_string": new}),
            ));
            assert!(out.silent(), "{old} -> {new}: {}", out.reason());
            assert_eq!(fake.calls(), 1);
        }
    }
}
