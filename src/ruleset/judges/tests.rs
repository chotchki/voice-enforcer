use std::path::{Path, PathBuf};
use std::time::Duration;

use super::*;
use crate::ruleset::{Ruleset, parse};

fn parse_at(dir: &Path, src: &str) -> Result<Ruleset, LoadError> {
    parse(src, &dir.join("ruleset.toml"), Path::new("/home/tester"))
}

fn ok(src: &str) -> Ruleset {
    parse_at(Path::new("/nowhere"), src).unwrap()
}

fn err(src: &str) -> LoadError {
    parse_at(Path::new("/nowhere"), src).unwrap_err()
}

const JUDGE: &str = r#"version = 1

[[judge]]
id = "no-recap"
severity = "warn"
scope = "diff"
rubric = """
  Flag a sentence that restates a point already made.
"""
"#;

#[test]
fn no_judges_means_defaults_and_none() {
    let rs = ok("version = 1\n");
    assert_eq!(rs.judges, vec![]);
    assert_eq!(rs.judge_settings, JudgeSettings::default());
    assert_eq!(rs.judge_settings.model, "haiku");
    assert_eq!(rs.judge_settings.effort, "medium");
    assert_eq!(rs.judge_settings.command, "claude");
    assert_eq!(rs.judge_settings.timeout, Duration::from_secs(45));
}

#[test]
fn inline_judge_parses() {
    let rs = ok(JUDGE);
    assert_eq!(
        rs.judges,
        vec![Judge {
            id: "no-recap".into(),
            severity: Severity::Warn,
            scope: Scope::Diff,
            registers: vec![Register::LongForm, Register::Comment],
            rubric: "Flag a sentence that restates a point already made.".into(),
        }]
    );
    assert!(rs.judges[0].applies_to(Register::Comment));
}

#[test]
fn rubric_file_resolves_next_to_the_ruleset() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("judges")).unwrap();
    std::fs::write(dir.path().join("judges/recap.md"), "From a file.\n").unwrap();
    let src = "version = 1\n[[judge]]\nid = \"j\"\nseverity = \"block\"\nscope = \"whole\"\nregisters = [\"long_form\"]\nrubric_file = \"judges/recap.md\"\n";
    let rs = parse_at(dir.path(), src).unwrap();
    let judge = &rs.judges[0];
    assert_eq!(judge.rubric, "From a file.");
    assert_eq!(judge.scope, Scope::Whole);
    assert_eq!(judge.severity, Severity::Block);
    assert_eq!(judge.registers, vec![Register::LongForm]);
}

#[test]
fn rubric_file_relative_to_a_bare_ruleset_name() {
    let got = parse(
        "version = 1\n[[judge]]\nid = \"j\"\nseverity = \"warn\"\nscope = \"diff\"\nrubric_file = \"surely-missing.md\"\n",
        Path::new("ruleset.toml"),
        Path::new("/h"),
    );
    assert!(got.unwrap_err().message.contains("surely-missing.md"));
}

#[test]
fn rubric_errors() {
    let base = "version = 1\n[[judge]]\nid = \"j\"\nseverity = \"warn\"\nscope = \"diff\"\n";
    for (extra, needle, line) in [
        ("", "judge `j`: needs `rubric` or `rubric_file`", Some(2)),
        (
            "rubric = \"x\"\nrubric_file = \"y.md\"\n",
            "not both",
            Some(7),
        ),
        (
            "rubric_file = \"missing.md\"\n",
            "can't read `rubric_file` `missing.md`",
            Some(6),
        ),
        ("rubric = \"   \"\n", "the rubric is empty", Some(6)),
    ] {
        let e = err(&format!("{base}{extra}"));
        assert!(e.message.contains(needle), "{extra}: {e}");
        assert_eq!(e.line, line, "{extra}: {e}");
    }
}

#[test]
fn empty_rubric_file_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("empty.md"), "\n\n").unwrap();
    let e = parse_at(
        dir.path(),
        "version = 1\n[[judge]]\nid = \"j\"\nseverity = \"warn\"\nscope = \"diff\"\nrubric_file = \"empty.md\"\n",
    )
    .unwrap_err();
    assert!(e.message.contains("the rubric is empty"), "{e}");
}

#[test]
fn field_errors() {
    for (body, needle) in [
        (
            "severity = \"warn\"\nscope = \"diff\"\nrubric = \"x\"",
            "judge is missing `id`",
        ),
        (
            "id = \"j\"\nscope = \"diff\"\nrubric = \"x\"",
            "judge `j`: missing `severity`",
        ),
        (
            "id = \"j\"\nseverity = \"warn\"\nrubric = \"x\"",
            "judge `j`: missing `scope`",
        ),
        (
            "id = \"j\"\nseverity = \"warn\"\nscope = \"file\"\nrubric = \"x\"",
            "unknown scope `file`; expected diff or whole",
        ),
        (
            "id = \"j\"\nseverity = \"warn\"\nscope = \"diff\"\nrubric = \"x\"\nregisters = []",
            "`registers` must not be empty",
        ),
        (
            "id = \"j\"\nseverity = \"warn\"\nscope = \"diff\"\nrubric = \"x\"\nmessage = \"m\"",
            "message",
        ),
    ] {
        let e = err(&format!("version = 1\n[[judge]]\n{body}\n"));
        assert!(e.message.contains(needle), "{body}: {e}");
    }
}

#[test]
fn ids_are_shared_with_rules() {
    let src = r#"version = 1
[[rule]]
id = "dup"
kind = "file_links"
severity = "warn"
message = "m"

[[judge]]
id = "dup"
severity = "warn"
scope = "diff"
rubric = "x"
"#;
    let e = err(src);
    assert!(e.message.contains("duplicate judge id `dup`"), "{e}");
    assert_eq!(e.line, Some(9));
}

#[test]
fn settings_parse_and_validate() {
    let rs = ok(
        "version = 1\n[judges]\nmodel = \"sonnet\"\neffort = \"low\"\ncommand = \"/opt/claude\"\ntimeout_secs = 10\n",
    );
    assert_eq!(
        rs.judge_settings,
        JudgeSettings {
            model: "sonnet".into(),
            effort: "low".into(),
            command: "/opt/claude".into(),
            timeout: Duration::from_secs(10),
        }
    );
    let rs = ok("version = 1\n[judges]\n");
    assert_eq!(rs.judge_settings, JudgeSettings::default());

    for (body, needle) in [
        ("effort = \"extreme\"", "unknown `judges.effort` `extreme`"),
        ("model = \" \"", "`judges.model` must not be empty"),
        ("command = \"\"", "`judges.command` must not be empty"),
        (
            "timeout_secs = 0",
            "`judges.timeout_secs` must be at least 1",
        ),
        ("timeouts = 3", "timeouts"),
    ] {
        let e = err(&format!("version = 1\n[judges]\n{body}\n"));
        assert!(e.message.contains(needle), "{body}: {e}");
        assert_eq!(e.line, Some(3), "{body}: {e}");
    }
}

#[test]
fn absolute_rubric_file_is_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let file: PathBuf = dir.path().join("abs.md");
    std::fs::write(&file, "Absolute.").unwrap();
    let rs = ok(&format!(
        "version = 1\n[[judge]]\nid = \"j\"\nseverity = \"warn\"\nscope = \"diff\"\nrubric_file = \"{}\"\n",
        file.display()
    ));
    assert_eq!(rs.judges[0].rubric, "Absolute.");
}
