use std::time::{Duration, Instant};

use serde_json::json;

use super::*;
use crate::judges::fake::{FakeClaude, success};
use crate::types::{Register, Severity};

fn judge() -> Judge {
    Judge {
        id: "no-recap".into(),
        severity: Severity::Warn,
        scope: crate::ruleset::Scope::Diff,
        registers: vec![Register::LongForm],
        rubric: "Flag recap sentences.".into(),
    }
}

fn settings(fake: &FakeClaude, timeout: u64) -> JudgeSettings {
    JudgeSettings {
        command: fake.command(),
        timeout: Duration::from_secs(timeout),
        ..JudgeSettings::default()
    }
}

#[test]
fn sends_the_exact_contract() {
    let fake = FakeClaude::answering(&json!([]));
    let verdict = call(&settings(&fake, 10), &judge(), "the input text").unwrap();
    assert_eq!(verdict, Verdict { findings: vec![] });

    let argv = fake.argv();
    let prompt = system_prompt(&judge());
    assert_eq!(
        argv,
        [
            "-p",
            "--safe-mode",
            "--model",
            "haiku",
            "--effort",
            "medium",
            "--tools",
            "",
            "--no-session-persistence",
            "--output-format",
            "json",
            "--json-schema",
            SCHEMA,
            "--system-prompt",
            prompt.as_str(),
        ]
    );
    assert!(prompt.starts_with("You are a prose judge."));
    assert!(prompt.ends_with("RUBRIC:\nFlag recap sentences."));
    assert!(fake.env().lines().any(|l| l == "VOICE_ENFORCER_JUDGE=1"));
    assert_eq!(fake.stdin(), "the input text");
}

#[test]
fn schema_is_valid_json() {
    let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
    assert_eq!(schema["required"], json!(["findings"]));
}

#[test]
fn reads_structured_output() {
    let fake = FakeClaude::answering(&json!([
        {"quote": "In conclusion, it works.", "explanation": "A recap.", "suggestion": "Cut it."},
        {"quote": "Also this.", "explanation": "Another."}
    ]));
    let verdict = call(&settings(&fake, 10), &judge(), "x").unwrap();
    assert_eq!(
        verdict.findings,
        vec![
            RawFinding {
                quote: "In conclusion, it works.".into(),
                explanation: "A recap.".into(),
                suggestion: Some("Cut it.".into()),
            },
            RawFinding {
                quote: "Also this.".into(),
                explanation: "Another.".into(),
                suggestion: None,
            },
        ]
    );
}

#[test]
fn falls_back_to_the_result_string() {
    let out = json!({
        "is_error": false,
        "result": "{\"findings\":[{\"quote\":\"q\",\"explanation\":\"e\"}]}"
    })
    .to_string();
    let fake = FakeClaude::new(&out, 0, 0.0);
    let verdict = call(&settings(&fake, 10), &judge(), "x").unwrap();
    assert_eq!(verdict.findings[0].quote, "q");
}

#[test]
fn failures_are_typed() {
    type Check = fn(&Failure) -> bool;
    let cases: Vec<(FakeClaude, Check)> = vec![
        (
            FakeClaude::new(
                &json!({"is_error": true, "subtype": "error_max_turns"}).to_string(),
                0,
                0.0,
            ),
            |f| matches!(f, Failure::Error(e) if e == "error_max_turns"),
        ),
        (
            FakeClaude::new(
                &json!({"is_error": true, "result": "overloaded"}).to_string(),
                0,
                0.0,
            ),
            |f| matches!(f, Failure::Error(e) if e == "overloaded"),
        ),
        (
            FakeClaude::new(&json!({"is_error": true}).to_string(), 0, 0.0),
            |f| matches!(f, Failure::Error(e) if e == "no detail"),
        ),
        (
            FakeClaude::new(&success(&json!([])), 3, 0.0),
            |f| matches!(f, Failure::Exit(Some(3), e) if e == "fake stderr line"),
        ),
        (
            FakeClaude::new(
                &json!({"is_error": true, "result": "Not logged in · Please run /login"})
                    .to_string(),
                1,
                0.0,
            ),
            |f| matches!(f, Failure::Error(e) if e == "Not logged in · Please run /login"),
        ),
        (
            FakeClaude::new("not json at all", 0, 0.0),
            |f| matches!(f, Failure::Unparseable(e) if e.contains("envelope")),
        ),
        (
            FakeClaude::new(
                &json!({"structured_output": {"nope": 1}}).to_string(),
                0,
                0.0,
            ),
            |f| matches!(f, Failure::Unparseable(_)),
        ),
    ];
    for (fake, check) in cases {
        let got = call(&settings(&fake, 10), &judge(), "x").unwrap_err();
        assert!(check(&got), "{got:?}");
    }
}

#[test]
fn missing_command_fails_to_spawn() {
    let settings = JudgeSettings {
        command: "/definitely/not/a/claude".into(),
        ..JudgeSettings::default()
    };
    let got = call(&settings, &judge(), "x").unwrap_err();
    assert!(
        matches!(&got, Failure::Spawn(e) if e.contains("/definitely/not/a/claude")),
        "{got:?}"
    );
}

#[test]
fn slow_judge_is_killed_at_the_timeout() {
    let fake = FakeClaude::new(&success(&json!([])), 0, 30.0);
    let start = Instant::now();
    let got = call(&settings(&fake, 1), &judge(), "x").unwrap_err();
    assert_eq!(got, Failure::Timeout(Duration::from_secs(1)));
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn large_input_does_not_deadlock() {
    let fake = FakeClaude::answering(&json!([]));
    let input = "x".repeat(2_000_000);
    call(&settings(&fake, 20), &judge(), &input).unwrap();
    assert_eq!(fake.stdin().len(), input.len());
}

#[test]
fn failure_messages() {
    assert_eq!(
        Failure::Spawn("`c`: nope".into()).to_string(),
        "couldn't start: `c`: nope"
    );
    assert_eq!(
        Failure::Timeout(Duration::from_secs(45)).to_string(),
        "timed out after 45s"
    );
    assert_eq!(
        Failure::Exit(Some(2), String::new()).to_string(),
        "exited with 2"
    );
    assert_eq!(
        Failure::Exit(None, "boom".into()).to_string(),
        "exited with a signal: boom"
    );
    assert_eq!(
        Failure::Error("x".into()).to_string(),
        "reported an error: x"
    );
    assert_eq!(
        Failure::Unparseable("y".into()).to_string(),
        "returned a verdict that didn't parse: y"
    );
}

#[test]
fn routed_fake_answers_per_marker() {
    let fake = FakeClaude::routed(
        &[(
            "recap",
            success(&json!([{"quote": "a", "explanation": "b"}])),
        )],
        &success(&json!([])),
        0,
        0.0,
    );
    assert_eq!(
        call(&settings(&fake, 10), &judge(), "x")
            .unwrap()
            .findings
            .len(),
        1
    );
    let mut other = judge();
    other.rubric = "Something else.".into();
    assert_eq!(
        call(&settings(&fake, 10), &other, "x")
            .unwrap()
            .findings
            .len(),
        0
    );
    assert_eq!(fake.calls(), 2);
}
