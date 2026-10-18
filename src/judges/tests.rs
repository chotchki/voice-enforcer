use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use serde_json::json;

use super::*;
use crate::extract::extract;
use crate::judges::fake::{FakeClaude, success};
use crate::ruleset::parse;
use crate::types::Severity;

fn ruleset(fake: &FakeClaude, judges: &str) -> Ruleset {
    parse(
        &format!(
            "version = 1\n[judges]\ncommand = \"{}\"\ntimeout_secs = 10\n{judges}",
            fake.command()
        ),
        Path::new("/nowhere/ruleset.toml"),
        Path::new("/h"),
    )
    .unwrap()
}

const TWO_JUDGES: &str = r#"
[[judge]]
id = "recap"
severity = "block"
scope = "diff"
registers = ["long_form"]
rubric = "RECAP_RUBRIC"

[[judge]]
id = "restates"
severity = "warn"
scope = "diff"
registers = ["comment"]
rubric = "RESTATES_RUBRIC"
"#;

const DOC: &str = "Intro line.\n\nIt works. In conclusion, it works.\n";

fn cache(dir: &Path) -> Cache {
    Cache::new(dir, SystemTime::now())
}

#[test]
fn findings_come_back_verified() {
    let fake = FakeClaude::routed(
        &[(
            "RECAP_RUBRIC",
            success(&json!([
                {"quote": "In conclusion, it works.", "explanation": "Recap."},
                {"quote": "Never said.", "explanation": "Invented."}
            ])),
        )],
        &success(&json!([])),
        0,
        0.0,
    );
    let rs = ruleset(&fake, TWO_JUDGES);
    let spans = extract(Path::new("a.md"), DOC, &[]);
    let dir = tempfile::tempdir().unwrap();
    let out = run(&rs, &spans, None, &cache(dir.path()));
    assert_eq!(out.notes, Vec::<String>::new());
    assert_eq!(out.findings.len(), 1);
    assert_eq!(out.findings[0].rule_id, "recap");
    assert_eq!(out.findings[0].severity, Severity::Block);
    assert_eq!(out.findings[0].line, 3);
    assert_eq!(fake.calls(), 1, "the comment judge had nothing to read");
}

#[test]
fn no_judges_or_nothing_changed_means_no_calls() {
    let fake = FakeClaude::answering(&json!([]));
    let spans = extract(Path::new("a.md"), DOC, &[]);
    let dir = tempfile::tempdir().unwrap();
    let changed: HashSet<usize> = [42].into();
    let out = run(
        &ruleset(&fake, TWO_JUDGES),
        &spans,
        Some(&changed),
        &cache(dir.path()),
    );
    assert_eq!(out, Outcome::default());
    let out = run(&ruleset(&fake, ""), &spans, None, &cache(dir.path()));
    assert_eq!(out, Outcome::default());
    assert_eq!(fake.calls(), 0);
}

#[test]
fn judges_run_in_parallel() {
    let fake = FakeClaude::new(&success(&json!([])), 0, 2.0);
    let judges = r#"
[[judge]]
id = "a"
severity = "warn"
scope = "diff"
rubric = "A"

[[judge]]
id = "b"
severity = "warn"
scope = "whole"
rubric = "B"
"#;
    let spans = extract(Path::new("a.md"), DOC, &[]);
    let dir = tempfile::tempdir().unwrap();
    let start = Instant::now();
    let out = run(&ruleset(&fake, judges), &spans, None, &cache(dir.path()));
    let took = start.elapsed();
    assert_eq!(out, Outcome::default());
    assert_eq!(fake.calls(), 2);
    assert!(took < Duration::from_millis(3500), "took {took:?}");
}

#[test]
fn identical_input_is_served_from_the_cache() {
    let fake = FakeClaude::answering(
        &json!([{"quote": "In conclusion, it works.", "explanation": "Recap."}]),
    );
    let rs = ruleset(&fake, TWO_JUDGES);
    let spans = extract(Path::new("a.md"), DOC, &[]);
    let dir = tempfile::tempdir().unwrap();
    let first = run(&rs, &spans, None, &cache(dir.path()));
    let second = run(&rs, &spans, None, &cache(dir.path()));
    assert_eq!(first, second);
    assert_eq!(fake.calls(), 1);

    let changed_rubric = ruleset(&fake, &TWO_JUDGES.replace("RECAP_RUBRIC", "NEW_RUBRIC"));
    run(&changed_rubric, &spans, None, &cache(dir.path()));
    assert_eq!(fake.calls(), 2, "a rubric change busts the cache");
}

#[test]
fn failures_become_notes_and_are_not_cached() {
    let fake = FakeClaude::new("garbage", 0, 0.0);
    let rs = ruleset(&fake, TWO_JUDGES);
    let spans = extract(Path::new("a.md"), DOC, &[]);
    let dir = tempfile::tempdir().unwrap();
    let out = run(&rs, &spans, None, &cache(dir.path()));
    assert_eq!(out.findings, vec![]);
    assert_eq!(out.notes.len(), 1);
    assert!(
        out.notes[0].starts_with("judge `recap` returned a verdict that didn't parse"),
        "{}",
        out.notes[0]
    );
    assert!(
        out.notes[0].ends_with("so it wasn't checked"),
        "{}",
        out.notes[0]
    );
    run(&rs, &spans, None, &cache(dir.path()));
    assert_eq!(fake.calls(), 2, "a failure is retried, not cached");
}

#[test]
fn oversized_input_is_a_note_not_a_call() {
    let fake = FakeClaude::answering(&json!([]));
    let big = format!("{}\n", "word ".repeat(input::MAX_INPUT_CHARS / 4));
    let spans = extract(Path::new("a.md"), &big, &[]);
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        &ruleset(&fake, TWO_JUDGES),
        &spans,
        None,
        &cache(dir.path()),
    );
    assert_eq!(out.notes.len(), 1);
    assert!(
        out.notes[0].contains("over the 200000 limit"),
        "{}",
        out.notes[0]
    );
    assert_eq!(fake.calls(), 0);
}
