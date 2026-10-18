use std::time::SystemTime;

use super::*;
use crate::extract::extract;
use crate::rules::evaluate;
use crate::ruleset::parse;
use crate::types::Severity;

const RULES: &str = r#"version = 1
[[rule]]
id = "no-leverage"
kind = "phrase"
severity = "block"
message = "Marketing verb"
suggestion = "say what it does"
phrases = ["leverage"]

[[judge]]
id = "recap"
severity = "warn"
scope = "diff"
rubric = "Flag recaps."
"#;

fn ruleset() -> Ruleset {
    parse(RULES, Path::new("/nowhere/ruleset.toml"), Path::new("/h")).unwrap()
}

fn rule_sample(path: &str, src: &str) -> Sample {
    let rs = ruleset();
    let path = Path::new(path);
    let spans = extract(path, src, &[]);
    let finding = evaluate(&rs, &spans, path).remove(0);
    Sample::new("0123abcd", &finding, path, &spans, &rs)
}

#[test]
fn rule_sample_holds_the_finding_and_its_prose() {
    let sample = rule_sample(
        "/r/README.md",
        "Intro.\n\nWe leverage the cache\nevery day.\n",
    );
    assert_eq!(
        sample,
        Sample {
            id: "0123abcd".into(),
            rule: "no-leverage".into(),
            kind: Kind::Rule,
            severity: "block".into(),
            file: "/r/README.md".into(),
            line: 3,
            matched: "leverage".into(),
            message: "Marketing verb".into(),
            suggestion: Some("say what it does".into()),
            model: None,
            effort: None,
            rubric_sha256: None,
            context: "We leverage the cache every day.".into(),
            code_context: None,
        }
    );
}

#[test]
fn comment_sample_carries_its_code_context() {
    let sample = rule_sample(
        "/r/lib.rs",
        "fn f() {\n    // leverage the counter\n    count += 1;\n}\n",
    );
    assert_eq!(sample.context, "leverage the counter");
    assert_eq!(sample.code_context.as_deref(), Some("    count += 1;\n}"));
}

#[test]
fn judge_sample_names_model_effort_and_rubric() {
    let rs = ruleset();
    let path = Path::new("/r/a.md");
    let spans = extract(path, "It works. In conclusion, it works.\n", &[]);
    let finding = Finding {
        rule_id: "recap".into(),
        severity: Severity::Warn,
        line: 1,
        end_line: 1,
        matched: "In conclusion, it works.".into(),
        message: "A recap.".into(),
        suggestion: None,
    };
    let sample = Sample::new("feedface", &finding, path, &spans, &rs);
    assert_eq!(sample.kind, Kind::Judge);
    assert_eq!(sample.model.as_deref(), Some("haiku"));
    assert_eq!(sample.effort.as_deref(), Some("medium"));
    let hash = sample.rubric_sha256.unwrap();
    assert_eq!(hash.len(), 16);
    assert_eq!(hash, short_sha256("Flag recaps."));
}

#[test]
fn finding_outside_every_span_has_empty_context() {
    let rs = ruleset();
    let finding = Finding {
        rule_id: "no-leverage".into(),
        severity: Severity::Block,
        line: 99,
        end_line: 99,
        matched: "x".into(),
        message: "m".into(),
        suggestion: None,
    };
    let sample = Sample::new("00000000", &finding, Path::new("/r/a.md"), &[], &rs);
    assert_eq!(sample.context, "");
    assert_eq!(sample.code_context, None);
}

#[test]
fn context_is_capped() {
    let long = format!("We leverage {}\n", "word ".repeat(1500));
    let sample = rule_sample("/r/a.md", &long);
    assert!(sample.context.ends_with('…'));
    assert_eq!(sample.context.chars().count(), MAX_CONTEXT_CHARS + 1);
}

#[test]
fn save_then_load() {
    let dir = tempfile::tempdir().unwrap();
    let sample = rule_sample("/r/README.md", "We leverage it.\n");
    save(dir.path(), std::slice::from_ref(&sample));
    assert_eq!(load(dir.path(), "0123abcd"), Some(sample));
    assert_eq!(load(dir.path(), "ffffffff"), None);
    assert_eq!(
        load(dir.path(), "../etc"),
        None,
        "ids are 8 hex chars, never a path"
    );
    assert_eq!(load(dir.path(), "0123abc"), None);
    save(dir.path(), &[]);
}

#[test]
fn corrupt_sample_is_unknown() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("findings")).unwrap();
    fs::write(dir.path().join("findings/0123abcd.json"), "{nope").unwrap();
    assert_eq!(load(dir.path(), "0123abcd"), None);
}

#[test]
fn unwritable_state_is_best_effort() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("findings"), "a file, not a dir").unwrap();
    let sample = rule_sample("/r/README.md", "We leverage it.\n");
    save(dir.path(), &[sample]);
    assert_eq!(load(dir.path(), "0123abcd"), None);
    prune(dir.path(), SystemTime::now());
}

#[test]
fn failed_rename_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("findings/0123abcd.json/inside")).unwrap();
    save(
        dir.path(),
        &[rule_sample("/r/README.md", "We leverage it.\n")],
    );
    let leftovers = fs::read_dir(dir.path().join("findings"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn old_samples_expire() {
    let dir = tempfile::tempdir().unwrap();
    save(
        dir.path(),
        &[rule_sample("/r/README.md", "We leverage it.\n")],
    );
    prune(dir.path(), SystemTime::now());
    assert!(load(dir.path(), "0123abcd").is_some());
    prune(
        dir.path(),
        SystemTime::now() + KEEP_FOR + Duration::from_secs(60),
    );
    assert_eq!(load(dir.path(), "0123abcd"), None);
}

mod command {
    use std::path::PathBuf;
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;
    use crate::test_support::run_with;

    /// 2026-10-17 19:30:05 UTC
    const NOW: u64 = 1_792_265_405;

    struct Fixture {
        dir: TempDir,
        env: Env,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().to_path_buf();
            let env = Env {
                home: root.join("home"),
                cwd: root.clone(),
                xdg_config_home: None,
                xdg_state_home: Some(root.join("state")),
                xdg_cache_home: None,
                ruleset_dir: Some(root.join("rules")),
                now: SystemTime::UNIX_EPOCH + Duration::from_secs(NOW),
                exe: PathBuf::from("/bin/voice-enforcer"),
                judge_child: false,
            };
            Self { dir, env }
        }

        fn sample(&self, id: &str, rule: &str, kind: Kind) -> Sample {
            let judge = kind == Kind::Judge;
            let sample = Sample {
                id: id.into(),
                rule: rule.into(),
                kind,
                severity: "warn".into(),
                file: "/r/docs/design.md".into(),
                line: 31,
                matched: "so the failure mode is a missed check".into(),
                message: "A recap.".into(),
                suggestion: judge.then(|| "Cut it.".into()),
                model: judge.then(|| "haiku".into()),
                effort: judge.then(|| "medium".into()),
                rubric_sha256: judge.then(|| "9b1c0f2a77e4d3c1".into()),
                context: "Line one of the paragraph.\nso the failure mode is a missed check."
                    .into(),
                code_context: None,
            };
            save(&self.env.state_dir(), std::slice::from_ref(&sample));
            sample
        }

        fn disputes(&self, rule: &str) -> Vec<PathBuf> {
            let mut found: Vec<_> = fs::read_dir(self.dir.path().join("rules/disputes").join(rule))
                .map(|d| d.flatten().map(|e| e.path()).collect())
                .unwrap_or_default();
            found.sort();
            found
        }
    }

    #[test]
    fn judge_dispute_records_everything() {
        let fx = Fixture::new();
        fx.sample("3fa9c201", "recap-closer", Kind::Judge);
        let out = run_with(
            &[
                "dispute",
                "3fa9c201",
                "--reason",
                " a real contrast, not a recap ",
            ],
            "",
            &fx.env,
        );
        assert_eq!(out.code, 0, "{}", out.stderr);
        let files = fx.disputes("recap-closer");
        assert_eq!(files.len(), 1);
        assert!(
            files[0].ends_with("20261017193005-3fa9c201.toml"),
            "{files:?}"
        );
        assert!(
            out.stdout.contains(&files[0].display().to_string()),
            "{}",
            out.stdout
        );
        insta::assert_snapshot!("judge_dispute_file", fs::read_to_string(&files[0]).unwrap());
    }

    #[test]
    fn dispute_files_parse_back() {
        let fx = Fixture::new();
        fx.sample("3fa9c201", "recap-closer", Kind::Judge);
        run_with(&["dispute", "3fa9c201", "--reason", "why"], "", &fx.env);
        let parsed: toml::Table =
            toml::from_str(&fs::read_to_string(&fx.disputes("recap-closer")[0]).unwrap()).unwrap();
        assert_eq!(parsed["version"].as_integer(), Some(1));
        assert_eq!(parsed["kind"].as_str(), Some("judge"));
        assert_eq!(parsed["disputed_at"].as_str(), Some("2026-10-17T19:30:05Z"));
        assert_eq!(
            parsed["context"].as_str(),
            Some("Line one of the paragraph.\nso the failure mode is a missed check.")
        );
    }

    #[test]
    fn rule_dispute_lands_under_the_rule() {
        let fx = Fixture::new();
        fx.sample("0000beef", "no-leverage", Kind::Rule);
        let out = run_with(
            &[
                "dispute",
                "0000beef",
                "--reason",
                "quoting the phrase on purpose",
            ],
            "",
            &fx.env,
        );
        assert_eq!(out.code, 0, "{}", out.stderr);
        let text = fs::read_to_string(&fx.disputes("no-leverage")[0]).unwrap();
        assert!(text.contains("kind = \"rule\""), "{text}");
        assert!(
            !text.contains("model"),
            "rule disputes carry no judge settings: {text}"
        );
    }

    #[test]
    fn blank_or_missing_reason_writes_nothing() {
        let fx = Fixture::new();
        fx.sample("3fa9c201", "recap-closer", Kind::Judge);
        let out = run_with(&["dispute", "3fa9c201", "--reason", "   "], "", &fx.env);
        assert_eq!(out.code, 2);
        assert!(out.stderr.contains("needs a reason"), "{}", out.stderr);
        let out = run_with(&["dispute", "3fa9c201"], "", &fx.env);
        assert_eq!(out.code, 2);
        assert_eq!(fx.disputes("recap-closer"), Vec::<PathBuf>::new());
    }

    #[test]
    fn unknown_id_writes_nothing() {
        let fx = Fixture::new();
        let out = run_with(&["dispute", "deadbeef", "--reason", "wrong"], "", &fx.env);
        assert_eq!(out.code, 2);
        assert!(
            out.stderr
                .contains("unknown or expired finding id `deadbeef`"),
            "{}",
            out.stderr
        );
        assert!(!fx.dir.path().join("rules/disputes").exists());
    }

    #[test]
    fn disputing_twice_writes_nothing_new() {
        let fx = Fixture::new();
        fx.sample("3fa9c201", "recap-closer", Kind::Judge);
        run_with(&["dispute", "3fa9c201", "--reason", "first"], "", &fx.env);
        let mut later = fx.env.clone();
        later.now += Duration::from_secs(3600);
        let out = run_with(&["dispute", "3fa9c201", "--reason", "second"], "", &later);
        assert_eq!(out.code, 0);
        let files = fx.disputes("recap-closer");
        assert_eq!(files.len(), 1);
        assert!(
            out.stdout.starts_with("already disputed:"),
            "{}",
            out.stdout
        );
        assert!(
            fs::read_to_string(&files[0])
                .unwrap()
                .contains("reason = \"first\"")
        );
    }

    #[test]
    fn ruleset_flag_picks_the_directory() {
        let fx = Fixture::new();
        fx.sample("3fa9c201", "recap-closer", Kind::Judge);
        let out = run_with(
            &[
                "dispute",
                "3fa9c201",
                "--reason",
                "why",
                "--ruleset",
                "elsewhere",
            ],
            "",
            &fx.env,
        );
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert!(
            fx.dir
                .path()
                .join("elsewhere/disputes/recap-closer")
                .is_dir()
        );
    }

    #[test]
    fn unwritable_ruleset_dir_exits_two() {
        let fx = Fixture::new();
        fx.sample("3fa9c201", "recap-closer", Kind::Judge);
        fs::create_dir_all(fx.dir.path().join("rules")).unwrap();
        fs::write(fx.dir.path().join("rules/disputes"), "a file, not a dir").unwrap();
        let out = run_with(&["dispute", "3fa9c201", "--reason", "why"], "", &fx.env);
        assert_eq!(out.code, 2);
        assert!(out.stderr.contains("can't write"), "{}", out.stderr);
    }

    #[test]
    fn existing_entry_with_the_id_counts_as_disputed() {
        let fx = Fixture::new();
        fx.sample("3fa9c201", "recap-closer", Kind::Judge);
        let target = fx
            .dir
            .path()
            .join("rules/disputes/recap-closer/20261017193005-3fa9c201.toml.d");
        fs::create_dir_all(&target).unwrap();
        fs::rename(&target, target.with_extension("")).unwrap();
        let out = run_with(&["dispute", "3fa9c201", "--reason", "why"], "", &fx.env);
        assert_eq!(
            out.code, 0,
            "an existing entry with that id counts as already disputed"
        );
        assert!(
            out.stdout.starts_with("already disputed:"),
            "{}",
            out.stdout
        );
    }

    #[test]
    fn hostile_rule_ids_stay_inside_disputes() {
        assert_eq!(safe_dir_name("no-leverage"), "no-leverage");
        assert_eq!(safe_dir_name("../../etc"), ".._.._etc");
        assert_eq!(safe_dir_name(".."), "_..");
        assert_eq!(safe_dir_name("a/b c"), "a_b_c");
        let fx = Fixture::new();
        fx.sample("3fa9c201", "../escape", Kind::Rule);
        run_with(&["dispute", "3fa9c201", "--reason", "why"], "", &fx.env);
        assert!(fx.dir.path().join("rules/disputes/.._escape").is_dir());
        assert!(!fx.dir.path().join("escape").exists());
    }
}
