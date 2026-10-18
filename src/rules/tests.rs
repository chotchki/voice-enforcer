use std::path::Path;

use super::*;
use crate::extract::extract;
use crate::ruleset::parse;

fn ruleset(rules: &str) -> Ruleset {
    parse(
        &format!("version = 1\n{rules}"),
        Path::new("ruleset.toml"),
        Path::new("/home/tester"),
    )
    .unwrap()
}

fn run(rules: &str, path: &str, src: &str) -> Vec<Finding> {
    let rs = ruleset(rules);
    let spans = extract(Path::new(path), src, &rs.comment_directives);
    evaluate(&rs, &spans, &Path::new("/nonexistent-repo").join(path))
}

const LEVERAGE: &str = r#"
[[rule]]
id = "no-leverage"
kind = "phrase"
severity = "block"
message = "Marketing verb"
suggestion = "use"
phrases = ["leverage", "at the end of the day"]
"#;

#[test]
fn phrase_hit_carries_the_full_finding() {
    assert_eq!(
        run(LEVERAGE, "README.md", "We Leverage the cache.\n"),
        vec![Finding {
            rule_id: "no-leverage".into(),
            severity: Severity::Block,
            line: 1,
            end_line: 1,
            matched: "Leverage".into(),
            message: "Marketing verb".into(),
            suggestion: Some("use".into()),
        }]
    );
}

#[test]
fn partial_words_do_not_match() {
    assert_eq!(run(LEVERAGE, "README.md", "the leveraged buyout\n"), vec![]);
}

#[test]
fn phrase_wrapped_across_lines() {
    let got = run(
        LEVERAGE,
        "README.md",
        "We got there at the end\nof the day.\n",
    );
    assert_eq!(got.len(), 1);
    assert_eq!((got[0].line, got[0].end_line), (1, 2));
    assert_eq!(got[0].matched, "at the end of the day");
}

#[test]
fn allow_when_drops_the_match() {
    let rules = r#"
[[rule]]
id = "no-seam"
kind = "phrase"
severity = "warn"
message = "Vague metaphor"
phrases = ["seam"]
allow_when = ["test seam"]
"#;
    assert_eq!(run(rules, "a.md", "Add a test seam here.\n"), vec![]);
    let got = run(rules, "a.md", "Add a test seam here. The seam is vague.\n");
    assert_eq!(got.len(), 1, "only the second sentence's hit survives");
}

#[test]
fn regex_rule_matches_and_skips_empty_matches() {
    let rules = r#"
[[rule]]
id = "very"
kind = "regex"
severity = "warn"
message = "Intensifier"
pattern = '(?i)\bvery (unique|important)\b'

[[rule]]
id = "empty"
kind = "regex"
severity = "warn"
message = "Never fires"
pattern = 'x*'
"#;
    let got = run(rules, "a.md", "a very important step\n");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].rule_id, "very");
    assert_eq!(got[0].matched, "very important");
}

#[test]
fn register_scoping() {
    let rules = r#"
[[rule]]
id = "long-only"
kind = "phrase"
severity = "warn"
message = "m"
registers = ["long_form"]
phrases = ["leverage"]
"#;
    assert_eq!(run(rules, "x.rs", "// leverage it\nfn f() {}\n"), vec![]);
    assert_eq!(run(rules, "x.md", "leverage it\n").len(), 1);
}

#[test]
fn masked_code_and_strings_never_match() {
    assert_eq!(
        run(
            LEVERAGE,
            "x.md",
            "Run `leverage` here.\n\n```\nleverage\n```\n"
        ),
        vec![]
    );
    assert_eq!(
        run(LEVERAGE, "x.rs", "fn f() { let s = \"leverage\"; }\n"),
        vec![]
    );
}

#[test]
fn oxford_comma_through_evaluate() {
    let rules = r#"
[[rule]]
id = "oxford"
kind = "oxford_comma"
severity = "warn"
message = "Drop the serial comma"
"#;
    let got = run(rules, "x.md", "Intro.\n\nWe like red, green, and blue.\n");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].line, 3);
    assert_eq!(got[0].matched, ",");
    assert_eq!(run(rules, "x.md", "We like red, green and blue.\n"), vec![]);
}

const LIMITS: &str = r#"
[[rule]]
id = "comment-length"
kind = "sentence_limit"
severity = "warn"
message = "Comment too long"
max_sentences = { line = 1, item = 2 }
"#;

#[test]
fn doc_comment_over_its_limit() {
    let src = "/// One. Two.\n/// Three.\nfn f() {}\n";
    let got = run(LIMITS, "x.rs", src);
    assert_eq!(got.len(), 1);
    assert_eq!((got[0].line, got[0].end_line), (1, 2));
    assert_eq!(
        got[0].message,
        "Comment too long (3 sentences, the item limit is 2)"
    );
    assert_eq!(got[0].matched, "One. Two. Three.");
}

#[test]
fn abbreviations_and_versions_do_not_split() {
    let src = "fn f() {\n    // Pins v1.2.3 of the client, e.g. for retries.\n}\n";
    assert_eq!(run(LIMITS, "x.rs", src), vec![]);
}

#[test]
fn level_without_a_limit_is_never_reported() {
    let src = "//! One. Two. Three. Four.\n";
    assert_eq!(run(LIMITS, "x.rs", src), vec![]);
}

#[test]
fn sentence_limit_ignores_long_form() {
    assert_eq!(run(LIMITS, "x.md", "One. Two. Three.\n"), vec![]);
}

#[test]
fn long_comment_preview_is_truncated() {
    let long = "word ".repeat(30);
    let src = format!("fn f() {{\n    // {long}. Two.\n}}\n");
    let got = run(LIMITS, "x.rs", &src);
    assert_eq!(got.len(), 1);
    assert!(got[0].matched.ends_with('…'), "{}", got[0].matched);
    assert_eq!(got[0].matched.chars().count(), 61);
}

#[test]
fn findings_are_ordered_by_line() {
    let rules = format!(
        "{LEVERAGE}\n[[rule]]\nid = \"z\"\nkind = \"phrase\"\nseverity = \"warn\"\nmessage = \"m\"\nphrases = [\"zebra\"]\n"
    );
    let got = run(&rules, "x.md", "zebra first.\n\nThen leverage.\n");
    let order: Vec<_> = got.iter().map(|f| (f.line, f.rule_id.as_str())).collect();
    assert_eq!(order, vec![(1, "z"), (3, "no-leverage")]);
}

#[test]
fn display_shows_masks_and_collapses_whitespace() {
    assert_eq!(display(&format!("a  {MASK}\n b")), "a `…` b");
}
