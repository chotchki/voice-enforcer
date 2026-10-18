use super::*;
use std::time::SystemTime;

const HOME: &str = "/home/tester";

fn parse_ok(src: &str) -> Ruleset {
    parse(src, Path::new("ruleset.toml"), Path::new(HOME)).unwrap()
}

fn parse_err(src: &str) -> LoadError {
    parse(src, Path::new("ruleset.toml"), Path::new(HOME)).unwrap_err()
}

fn with_rule(body: &str) -> String {
    format!("version = 1\n\n[[rule]]\n{body}\n")
}

const PHRASE: &str = r#"id = "no-leverage"
kind = "phrase"
severity = "block"
message = "Marketing verb"
phrases = ["leverage", "at the end of the day"]"#;

fn env() -> Env {
    Env {
        home: PathBuf::from(HOME),
        cwd: PathBuf::from("/work"),
        xdg_config_home: None,
        xdg_state_home: None,
        xdg_cache_home: None,
        ruleset_dir: None,
        now: SystemTime::UNIX_EPOCH,
        exe: PathBuf::from("/bin/voice-enforcer"),
        judge_child: false,
    }
}

#[test]
fn locate_falls_back_to_dot_config() {
    assert_eq!(
        locate(None, &env()),
        PathBuf::from("/home/tester/.config/voice-enforcer/ruleset.toml")
    );
}

#[test]
fn locate_prefers_xdg_over_dot_config() {
    let mut e = env();
    e.xdg_config_home = Some(PathBuf::from("/xdg"));
    assert_eq!(
        locate(None, &e),
        PathBuf::from("/xdg/voice-enforcer/ruleset.toml")
    );
}

#[test]
fn locate_prefers_env_over_xdg() {
    let mut e = env();
    e.xdg_config_home = Some(PathBuf::from("/xdg"));
    e.ruleset_dir = Some(PathBuf::from("/private/rules"));
    assert_eq!(
        locate(None, &e),
        PathBuf::from("/private/rules/ruleset.toml")
    );
}

#[test]
fn locate_prefers_flag_over_env_and_resolves_relative_to_cwd() {
    let mut e = env();
    e.ruleset_dir = Some(PathBuf::from("./b"));
    assert_eq!(
        locate(Some(Path::new("a")), &e),
        PathBuf::from("/work/a/ruleset.toml")
    );
}

#[test]
fn load_missing_file_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let got = load(&dir.path().join(FILE_NAME), Path::new(HOME)).unwrap();
    assert!(got.is_none());
}

#[test]
fn load_unreadable_path_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let err = load(dir.path(), Path::new(HOME)).unwrap_err();
    assert!(err.message.starts_with("can't read the ruleset"), "{err}");
    assert_eq!(err.line, None);
}

#[test]
fn load_reads_a_real_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    std::fs::write(&path, with_rule(PHRASE)).unwrap();
    let rs = load(&path, Path::new(HOME)).unwrap().unwrap();
    assert_eq!(rs.rules.len(), 1);
}

#[test]
fn minimal_ruleset_gets_defaults() {
    let rs = parse_ok("version = 1\n");
    assert!(rs.rules.is_empty());
    assert_eq!(rs.max_consecutive_blocks, 3);
    assert!(rs.comment_directives.is_empty());
}

#[test]
fn missing_version_names_supported_version() {
    let err = parse_err("");
    assert_eq!(err.line, None);
    assert!(err.message.contains("missing `version`"), "{err}");
    assert!(err.message.contains("version 1"), "{err}");
}

#[test]
fn future_version_names_supported_version() {
    let err = parse_err("\nversion = 2\n");
    assert_eq!(err.line, Some(2));
    assert!(err.message.contains("unsupported version 2"), "{err}");
    assert!(err.message.contains("supports version 1"), "{err}");
}

#[test]
fn toml_syntax_error_has_a_line() {
    let err = parse_err("version = 1\nexclude = [\n");
    assert!(err.line.is_some(), "{err}");
}

#[test]
fn unknown_top_level_key_is_rejected() {
    let err = parse_err("version = 1\nexclued = []\n");
    assert_eq!(err.line, Some(2));
    assert!(err.message.contains("exclued"), "{err}");
}

#[test]
fn typo_in_rule_key_names_key_and_line() {
    let src = with_rule(
        r#"id = "no-hype"
kind = "phrase"
severty = "block""#,
    );
    let err = parse_err(&src);
    assert!(err.message.contains("severty"), "{err}");
    assert_eq!(err.line, Some(6));
}

#[test]
fn typo_in_loop_guard_is_rejected() {
    let err = parse_err("version = 1\n[loop_guard]\nmax_blocks = 2\n");
    assert!(err.message.contains("max_blocks"), "{err}");
}

#[test]
fn duplicate_id_is_named() {
    let src = format!(
        "version = 1\n[[rule]]\n{PHRASE}\n[[rule]]\n{}\n",
        PHRASE.replace("Marketing verb", "again")
    );
    let err = parse_err(&src);
    assert!(
        err.message.contains("duplicate rule id `no-leverage`"),
        "{err}"
    );
    assert_eq!(err.line, Some(9));
}

#[test]
fn missing_fields_are_named() {
    for (body, needle) in [
        ("kind = \"file_links\"", "missing `id`"),
        ("id = \"  \"\nkind = \"file_links\"", "missing `id`"),
        ("id = \"a\"", "rule `a`: missing `kind`"),
        ("id = \"a\"\nkind = \"file_links\"", "missing `severity`"),
        (
            "id = \"a\"\nkind = \"file_links\"\nseverity = \"warn\"",
            "missing `message`",
        ),
    ] {
        let err = parse_err(&with_rule(body));
        assert!(err.message.contains(needle), "{body}: {err}");
        assert_eq!(err.line, Some(3), "{body}: {err}");
    }
}

#[test]
fn unknown_kind_lists_the_kinds() {
    let err = parse_err(&with_rule(
        "id = \"a\"\nkind = \"phrses\"\nseverity = \"warn\"\nmessage = \"m\"",
    ));
    assert!(err.message.contains("unknown kind `phrses`"), "{err}");
    assert!(err.message.contains("oxford_comma"), "{err}");
    assert_eq!(err.line, Some(5));
}

#[test]
fn unknown_severity_is_rejected() {
    let err = parse_err(&with_rule(
        "id = \"a\"\nkind = \"file_links\"\nseverity = \"blok\"\nmessage = \"m\"",
    ));
    assert!(err.message.contains("unknown severity `blok`"), "{err}");
    assert_eq!(err.line, Some(6));
}

#[test]
fn registers_parse_and_validate() {
    let rs = parse_ok(&with_rule(&format!(
        "{PHRASE}\nregisters = [\"long_form\"]"
    )));
    assert_eq!(rs.rules[0].registers, vec![Register::LongForm]);
    assert!(rs.rules[0].applies_to(Register::LongForm));
    assert!(!rs.rules[0].applies_to(Register::Comment));

    let err = parse_err(&with_rule(&format!("{PHRASE}\nregisters = []")));
    assert!(err.message.contains("must not be empty"), "{err}");

    let err = parse_err(&with_rule(&format!("{PHRASE}\nregisters = [\"comments\"]")));
    assert!(err.message.contains("unknown register `comments`"), "{err}");
    assert_eq!(err.line, Some(9));
}

#[test]
fn default_registers_follow_the_kind() {
    let src = r#"version = 1
[[rule]]
id = "p"
kind = "phrase"
severity = "warn"
message = "m"
phrases = ["x"]
[[rule]]
id = "s"
kind = "sentence_limit"
severity = "warn"
message = "m"
max_sentences = { line = 1 }
[[rule]]
id = "f"
kind = "file_links"
severity = "block"
message = "m"
"#;
    let rs = parse_ok(src);
    assert_eq!(
        rs.rules[0].registers,
        vec![Register::LongForm, Register::Comment]
    );
    assert_eq!(rs.rules[1].registers, vec![Register::Comment]);
    assert_eq!(rs.rules[2].registers, vec![Register::LongForm]);
}

#[test]
fn fields_must_belong_to_the_kind() {
    let err = parse_err(&with_rule(&format!("{PHRASE}\npattern = \"x\"")));
    assert!(
        err.message
            .contains("`pattern` is not valid for kind `phrase`"),
        "{err}"
    );
    assert_eq!(err.line, Some(9));

    let err = parse_err(&with_rule(
        "id = \"s\"\nkind = \"sentence_limit\"\nseverity = \"warn\"\nmessage = \"m\"\nmax_sentences = { line = 1 }\nregisters = [\"comment\"]",
    ));
    assert!(err.message.contains("`registers` is not valid"), "{err}");
}

#[test]
fn phrase_rule_compiles_whole_word_case_insensitive() {
    let rs = parse_ok(&with_rule(PHRASE));
    let rule = &rs.rules[0];
    assert_eq!(rule.id, "no-leverage");
    assert_eq!(rule.severity, Severity::Block);
    assert_eq!(rule.message, "Marketing verb");
    assert_eq!(rule.suggestion, None);
    let RuleKind::Phrase {
        phrases,
        allow_when,
    } = &rule.kind
    else {
        panic!("not a phrase rule");
    };
    assert!(allow_when.is_empty());
    assert!(phrases[0].is_match("We Leverage the cache."));
    assert!(!phrases[0].is_match("the leveraged buyout"));
    assert!(phrases[1].is_match("at the end\n  of the day"));
}

#[test]
fn phrase_without_word_edges_has_no_boundaries() {
    let rs = parse_ok(&with_rule(
        "id = \"p\"\nkind = \"phrase\"\nseverity = \"warn\"\nmessage = \"m\"\nphrases = [\"--> \", \"!!\"]",
    ));
    let RuleKind::Phrase { phrases, .. } = &rs.rules[0].kind else {
        panic!("not a phrase rule");
    };
    assert!(phrases[0].is_match("a-->b"));
    assert!(phrases[1].is_match("wow!!"));
}

#[test]
fn phrase_rule_needs_phrases() {
    let base = "id = \"p\"\nkind = \"phrase\"\nseverity = \"warn\"\nmessage = \"m\"";
    let err = parse_err(&with_rule(base));
    assert!(err.message.contains("non-empty `phrases`"), "{err}");
    let err = parse_err(&with_rule(&format!("{base}\nphrases = []")));
    assert!(err.message.contains("non-empty `phrases`"), "{err}");
    let err = parse_err(&with_rule(&format!("{base}\nphrases = [\"ok\", \"   \"]")));
    assert!(err.message.contains("empty phrase"), "{err}");
    assert_eq!(err.line, Some(8));
}

#[test]
fn allow_when_compiles_and_rejects_bad_regex() {
    let rs = parse_ok(&with_rule(&format!(
        "{PHRASE}\nallow_when = [\"test seam\"]\nsuggestion = \"use\""
    )));
    let RuleKind::Phrase { allow_when, .. } = &rs.rules[0].kind else {
        panic!("not a phrase rule");
    };
    assert!(allow_when[0].is_match("add a test seam here"));
    assert_eq!(rs.rules[0].suggestion.as_deref(), Some("use"));

    let err = parse_err(&with_rule(&format!("{PHRASE}\nallow_when = [\"(\"]")));
    assert!(
        err.message
            .contains("invalid regex in rule `no-leverage`: `allow_when`"),
        "{err}"
    );
    assert_eq!(err.line, Some(9));
}

#[test]
fn regex_rule() {
    let base = "id = \"r\"\nkind = \"regex\"\nseverity = \"warn\"\nmessage = \"m\"";
    let rs = parse_ok(&with_rule(&format!(
        "{base}\npattern = '(?i)\\bvery (unique|important)\\b'"
    )));
    let RuleKind::Regex { pattern } = &rs.rules[0].kind else {
        panic!("not a regex rule");
    };
    assert!(pattern.is_match("a very important step"));

    let err = parse_err(&with_rule(base));
    assert!(err.message.contains("needs a `pattern`"), "{err}");

    let err = parse_err(&with_rule(&format!("{base}\npattern = \"[\"")));
    assert!(err.message.contains("invalid regex"), "{err}");
    assert_eq!(err.line, Some(8));
}

#[test]
fn oxford_comma_rule() {
    let base = "id = \"o\"\nkind = \"oxford_comma\"\nseverity = \"warn\"\nmessage = \"m\"";
    let rs = parse_ok(&with_rule(base));
    assert!(matches!(
        rs.rules[0].kind,
        RuleKind::OxfordComma { max_item_words: 4 }
    ));
    let rs = parse_ok(&with_rule(&format!("{base}\nmax_item_words = 6")));
    assert!(matches!(
        rs.rules[0].kind,
        RuleKind::OxfordComma { max_item_words: 6 }
    ));
    let err = parse_err(&with_rule(&format!("{base}\nmax_item_words = 0")));
    assert!(
        err.message.contains("`max_item_words` must be at least 1"),
        "{err}"
    );
}

#[test]
fn sentence_limit_rule() {
    let base = "id = \"s\"\nkind = \"sentence_limit\"\nseverity = \"warn\"\nmessage = \"m\"";
    let rs = parse_ok(&with_rule(&format!(
        "{base}\nmax_sentences = {{ line = 1, item = 2, module = 5 }}"
    )));
    let RuleKind::SentenceLimit { limits } = rs.rules[0].kind else {
        panic!("not a sentence rule");
    };
    assert_eq!(limits.for_level(Level::Line), Some(1));
    assert_eq!(limits.for_level(Level::Item), Some(2));
    assert_eq!(limits.for_level(Level::Module), Some(5));

    let err = parse_err(&with_rule(base));
    assert!(err.message.contains("needs `max_sentences`"), "{err}");

    let err = parse_err(&with_rule(&format!("{base}\nmax_sentences = {{}}")));
    assert!(err.message.contains("at least one of line"), "{err}");

    let err = parse_err(&with_rule(&format!(
        "{base}\nmax_sentences = {{ item = 0 }}"
    )));
    assert!(
        err.message
            .contains("`max_sentences.item` must be at least 1"),
        "{err}"
    );

    let err = parse_err(&with_rule(&format!(
        "{base}\nmax_sentences = {{ lines = 1 }}"
    )));
    assert!(err.message.contains("lines"), "{err}");
}

#[test]
fn loop_guard_setting() {
    let rs = parse_ok("version = 1\n[loop_guard]\nmax_consecutive_blocks = 1\n");
    assert_eq!(rs.max_consecutive_blocks, 1);
    let rs = parse_ok("version = 1\n[loop_guard]\n");
    assert_eq!(rs.max_consecutive_blocks, 3);
    let err = parse_err("version = 1\n[loop_guard]\nmax_consecutive_blocks = 0\n");
    assert!(err.message.contains("at least 1"), "{err}");
    assert_eq!(err.line, Some(3));
}

#[test]
fn comment_directives_compile() {
    let rs = parse_ok("version = 1\ncomment_directives = [\"^lint:allow\"]\n");
    assert!(rs.comment_directives[0].is_match("lint:allow long-line"));
    let err = parse_err("version = 1\ncomment_directives = [\"(\"]\n");
    assert!(
        err.message
            .contains("invalid regex in `comment_directives`"),
        "{err}"
    );
    assert_eq!(err.line, Some(2));
}

#[test]
fn builtin_excludes() {
    let rs = parse_ok("version = 1\n");
    for path in [
        "/r/node_modules/x/README.md",
        "/r/Cargo.lock",
        "/r/app.min.js",
        "/r/target/debug/x.rs",
        "/r/.git/COMMIT_EDITMSG",
        "/r/vendor/lib.py",
        "/r/dist/index.js",
    ] {
        assert!(rs.is_excluded(Path::new(path)), "{path}");
    }
    for path in ["/r/README.md", "/r/build/census.sh", "/r/src/main.rs"] {
        assert!(!rs.is_excluded(Path::new(path)), "{path}");
    }
}

#[test]
fn configured_excludes_expand_home() {
    let rs =
        parse_ok("version = 1\nexclude = [\"~/workspace/their-repo/**\", \"/abs/*.md\", \"~\"]\n");
    assert!(rs.is_excluded(Path::new("/home/tester/workspace/their-repo/src/lib.rs")));
    assert!(!rs.is_excluded(Path::new("/home/tester/workspace/mine/src/lib.rs")));
    assert!(rs.is_excluded(Path::new("/abs/notes.md")));
    assert!(
        !rs.is_excluded(Path::new("/abs/deeper/notes.md")),
        "* doesn't cross /"
    );
    assert!(rs.is_excluded(Path::new("/home/tester")));
}

#[test]
fn invalid_glob_is_named() {
    let err = parse_err("version = 1\nexclude = [\"a/[\"]\n");
    assert!(err.message.contains("invalid glob `a/[`"), "{err}");
    assert_eq!(err.line, Some(2));
}

#[test]
fn load_error_display() {
    let with = LoadError {
        path: PathBuf::from("r.toml"),
        line: Some(4),
        message: "bad".into(),
    };
    assert_eq!(with.to_string(), "r.toml:4: bad");
    let without = LoadError { line: None, ..with };
    assert_eq!(without.to_string(), "r.toml: bad");
}

#[test]
fn line_of_counts_newlines() {
    assert_eq!(line_of("a\nb\nc", 0), 1);
    assert_eq!(line_of("a\nb\nc", 2), 2);
    assert_eq!(line_of("a\nb\nc", 99), 3);
}
