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
    let out = run(
        &rs,
        Path::new("/r/a.md"),
        &spans,
        &Changes::all(),
        &cache(dir.path()),
    );
    assert_eq!(out.notes, Vec::<String>::new());
    assert_eq!(out.findings.len(), 1);
    assert_eq!(out.findings[0].rule_id, "recap");
    assert_eq!(out.findings[0].severity, Severity::Block);
    assert_eq!(out.findings[0].line, 3);
    assert_eq!(fake.calls(), 1, "the comment judge had nothing to read");
}

#[test]
fn quotes_must_hold_a_changed_word() {
    const WHOLE: &str = r#"
[[judge]]
id = "repeats"
severity = "warn"
scope = "whole"
registers = ["long_form"]
rubric = "R"
"#;
    let before = "Old line stays.\n\nThe cache is warm. It is fast.\n";
    let after = "Old line stays.\n\nThe cache is warm. It is quick.\n";
    let changes = Changes::between(Some(before), after);
    let spans = extract(Path::new("a.md"), after, &[]);
    let quotes = json!([
        {"quote": "The cache is warm.", "explanation": "Old sentence."},
        {"quote": "It is quick.", "explanation": "New sentence."},
        {"quote": "Old line stays.", "explanation": "Untouched line."}
    ]);
    for (judges, kept) in [
        (TWO_JUDGES, vec!["It is quick."]),
        (WHOLE, vec!["It is quick."]),
    ] {
        let fake = FakeClaude::answering(&quotes);
        let dir = tempfile::tempdir().unwrap();
        let out = run(
            &ruleset(&fake, judges),
            Path::new("/r/a.md"),
            &spans,
            &changes,
            &cache(dir.path()),
        );
        let got: Vec<&str> = out.findings.iter().map(|f| f.matched.as_str()).collect();
        assert_eq!(got, kept, "{judges}");
    }
}

#[test]
fn no_judges_or_nothing_changed_means_no_calls() {
    let fake = FakeClaude::answering(&json!([]));
    let spans = extract(Path::new("a.md"), DOC, &[]);
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        &ruleset(&fake, TWO_JUDGES),
        Path::new("/r/a.md"),
        &spans,
        &Changes::on_lines(&[42]),
        &cache(dir.path()),
    );
    assert_eq!(out, Outcome::default());
    let out = run(
        &ruleset(&fake, ""),
        Path::new("/r/a.md"),
        &spans,
        &Changes::all(),
        &cache(dir.path()),
    );
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
    let out = run(
        &ruleset(&fake, judges),
        Path::new("/r/a.md"),
        &spans,
        &Changes::all(),
        &cache(dir.path()),
    );
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
    let first = run(
        &rs,
        Path::new("/r/a.md"),
        &spans,
        &Changes::all(),
        &cache(dir.path()),
    );
    let second = run(
        &rs,
        Path::new("/r/a.md"),
        &spans,
        &Changes::all(),
        &cache(dir.path()),
    );
    assert_eq!(first, second);
    assert_eq!(fake.calls(), 1);

    let changed_rubric = ruleset(&fake, &TWO_JUDGES.replace("RECAP_RUBRIC", "NEW_RUBRIC"));
    run(
        &changed_rubric,
        Path::new("/r/a.md"),
        &spans,
        &Changes::all(),
        &cache(dir.path()),
    );
    assert_eq!(fake.calls(), 2, "a rubric change busts the cache");
}

#[test]
fn failures_become_notes_and_are_not_cached() {
    let fake = FakeClaude::new("garbage", 0, 0.0);
    let rs = ruleset(&fake, TWO_JUDGES);
    let spans = extract(Path::new("a.md"), DOC, &[]);
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        &rs,
        Path::new("/r/a.md"),
        &spans,
        &Changes::all(),
        &cache(dir.path()),
    );
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
    run(
        &rs,
        Path::new("/r/a.md"),
        &spans,
        &Changes::all(),
        &cache(dir.path()),
    );
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
        Path::new("/r/a.md"),
        &spans,
        &Changes::all(),
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

#[test]
fn a_judge_skips_excluded_paths() {
    let fake = FakeClaude::answering(&json!([]));
    let judges = TWO_JUDGES.replace(
        "rubric = \"RECAP_RUBRIC\"",
        "rubric = \"RECAP_RUBRIC\"\nexclude = [\"/r/skip/**\"]",
    );
    let rs = ruleset(&fake, &judges);
    let spans = extract(Path::new("a.md"), DOC, &[]);
    let dir = tempfile::tempdir().unwrap();
    run(
        &rs,
        Path::new("/r/skip/a.md"),
        &spans,
        &Changes::all(),
        &cache(dir.path()),
    );
    assert_eq!(fake.calls(), 0);
    run(
        &rs,
        Path::new("/r/keep/a.md"),
        &spans,
        &Changes::all(),
        &cache(dir.path()),
    );
    assert_eq!(fake.calls(), 1);
}

#[test]
fn a_judges_own_effort_reaches_only_its_call() {
    let spans = extract(Path::new("a.md"), DOC, &[]);
    for (extra, want) in [("\neffort = \"high\"", "high"), ("", "medium")] {
        let fake = FakeClaude::answering(&json!([]));
        let judges = TWO_JUDGES.replace(
            "rubric = \"RECAP_RUBRIC\"",
            &format!("rubric = \"RECAP_RUBRIC\"{extra}"),
        );
        let dir = tempfile::tempdir().unwrap();
        run(
            &ruleset(&fake, &judges),
            Path::new("/r/a.md"),
            &spans,
            &Changes::all(),
            &cache(dir.path()),
        );
        let argv = fake.argv();
        let at = argv.iter().position(|a| a == "--effort").unwrap();
        assert_eq!(argv[at + 1], want);
    }
}
