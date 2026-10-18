use std::path::PathBuf;
use std::time::SystemTime;

use serde_json::Value;
use tempfile::TempDir;

use crate::Env;
use crate::test_support::{Output, run_with};

const RULES: &str = r#"version = 1
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
    dir: TempDir,
    env: Env,
}

impl Fixture {
    fn new(rules: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("rules")).unwrap();
        std::fs::create_dir_all(root.join("work")).unwrap();
        if let Some(rules) = rules {
            std::fs::write(root.join("rules/ruleset.toml"), rules).unwrap();
        }
        let env = Env {
            home: root.join("home"),
            cwd: root.join("work"),
            xdg_config_home: None,
            xdg_state_home: None,
            ruleset_dir: Some(root.join("rules")),
            now: SystemTime::now(),
            exe: PathBuf::from("/bin/voice-enforcer"),
        };
        Self { dir, env }
    }

    fn file(&self, rel: &str, content: &str) {
        let path = self.env.cwd.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        run_with(args, "", &self.env)
    }
}

#[test]
fn clean_tree_prints_nothing() {
    let fx = Fixture::new(Some(RULES));
    fx.file("README.md", "Plain words.\n");
    let out = fx.run(&["check", "."]);
    assert_eq!((out.code, out.stdout.as_str()), (0, ""));
}

#[test]
fn blocking_finding_exits_one() {
    let fx = Fixture::new(Some(RULES));
    fx.file("docs/a.md", "We leverage it.\n");
    let out = fx.run(&["check", "."]);
    assert_eq!(out.code, 1);
    assert_eq!(
        out.stdout,
        "docs/a.md:1: block no-leverage: Marketing verb (\"leverage\")\n    suggestion: say what it does\n"
    );
}

#[test]
fn warnings_alone_pass() {
    let fx = Fixture::new(Some(RULES));
    fx.file("a.md", "A very fine day.\n");
    let out = fx.run(&["check", "a.md"]);
    assert_eq!(out.code, 0);
    assert_eq!(out.stdout, "a.md:1: warn no-very: Intensifier (\"very\")\n");
}

#[test]
fn json_output() {
    let fx = Fixture::new(Some(RULES));
    fx.file("README.md", "We leverage a very big cache.\n");
    let out = fx.run(&["check", "--format", "json", "README.md"]);
    assert_eq!(out.code, 1);
    let list: Value = serde_json::from_str(&out.stdout).unwrap();
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["path"], "README.md");
    assert_eq!(list[0]["line"], 1);
    assert_eq!(list[0]["end_line"], 1);
    assert_eq!(list[0]["severity"], "block");
    assert_eq!(list[0]["rule_id"], "no-leverage");
    assert_eq!(list[0]["matched"], "leverage");
    assert_eq!(list[0]["message"], "Marketing verb");
    assert_eq!(list[0]["suggestion"], "say what it does");
    assert_eq!(list[1]["suggestion"], Value::Null);
}

#[test]
fn walk_skips_ignored_excluded_and_unknown_files() {
    let fx = Fixture::new(Some(RULES));
    fx.file(".gitignore", "out/\n");
    fx.file("out/gen.md", "leverage\n");
    fx.file("node_modules/x/README.md", "leverage\n");
    fx.file("data.csv", "leverage\n");
    fx.file("bin.md", "");
    std::fs::write(fx.env.cwd.join("bin.md"), [0xff, 0xfe]).unwrap();
    fx.file("src/lib.rs", "// We leverage this.\nfn f() {}\n");
    let out = fx.run(&["check", "."]);
    assert_eq!(out.code, 1);
    assert_eq!(out.stdout.lines().count(), 2, "{}", out.stdout);
    assert!(out.stdout.starts_with("src/lib.rs:1:"), "{}", out.stdout);
}

#[test]
fn paths_outside_cwd_print_absolute() {
    let fx = Fixture::new(Some(RULES));
    let outside = fx.dir.path().join("outside.md");
    std::fs::write(&outside, "leverage\n").unwrap();
    let out = fx.run(&["check", outside.to_str().unwrap()]);
    assert!(
        out.stdout.starts_with(&format!("{}:1:", outside.display())),
        "{}",
        out.stdout
    );
}

#[test]
fn usage_and_run_errors_exit_two() {
    let fx = Fixture::new(Some(RULES));
    let out = fx.run(&["check"]);
    assert_eq!(out.code, 2, "paths are required");
    let out = fx.run(&["check", "missing.md"]);
    assert_eq!(out.code, 2);
    assert!(
        out.stderr.contains("can't read missing.md"),
        "{}",
        out.stderr
    );
    let out = fx.run(&["check", "--format", "xml", "."]);
    assert_eq!(out.code, 2);
}

#[test]
fn missing_or_broken_ruleset_exits_two() {
    let fx = Fixture::new(None);
    fx.file("a.md", "x\n");
    let out = fx.run(&["check", "a.md"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("no ruleset at"), "{}", out.stderr);

    let fx = Fixture::new(Some("version = 3\n"));
    fx.file("a.md", "x\n");
    let out = fx.run(&["check", "a.md"]);
    assert_eq!(out.code, 2);
    assert!(
        out.stderr.contains("unsupported version 3"),
        "{}",
        out.stderr
    );
}

#[test]
fn ruleset_flag_overrides_env() {
    let fx = Fixture::new(None);
    let other = fx.dir.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("ruleset.toml"), RULES).unwrap();
    fx.file("a.md", "leverage\n");
    let out = fx.run(&["check", "--ruleset", other.to_str().unwrap(), "a.md"]);
    assert_eq!(out.code, 1);
}

#[test]
fn validate_reports_rule_count() {
    let fx = Fixture::new(Some(RULES));
    let out = fx.run(&["validate"]);
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("valid, 2 rule(s)"), "{}", out.stdout);
    assert!(out.stdout.contains("ruleset.toml"), "{}", out.stdout);
}

#[test]
fn validate_missing_or_invalid() {
    let fx = Fixture::new(None);
    let out = fx.run(&["validate"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("no ruleset at"), "{}", out.stderr);
    assert!(out.stderr.contains("rules/ruleset.toml"), "{}", out.stderr);

    let fx = Fixture::new(Some("version = 1\nbogus = 1\n"));
    let out = fx.run(&["validate"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("bogus"), "{}", out.stderr);
}
