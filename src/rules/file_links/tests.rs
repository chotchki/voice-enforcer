use std::path::Path;

use tempfile::TempDir;

use super::*;
use crate::extract::extract;
use crate::rules::evaluate;
use crate::ruleset::parse;

const RULE: &str = r#"version = 1
[[rule]]
id = "link-files"
kind = "file_links"
severity = "block"
message = "Link the files you name"
"#;

fn ruleset() -> Ruleset {
    parse(RULE, Path::new("ruleset.toml"), Path::new("/home/tester")).unwrap()
}

/// A temp git repo holding `files` (each with placeholder content).
fn repo(files: &[&str]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    for f in files {
        let path = dir.path().join(f);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "x").unwrap();
    }
    dir
}

fn check_md(root: &Path, md: &str, src: &str) -> Vec<Finding> {
    let rs = ruleset();
    let path = root.join(md);
    evaluate(&rs, &extract(&path, src, &[]), &path)
}

fn summary(findings: &[Finding]) -> Vec<(Severity, String, Option<String>)> {
    findings
        .iter()
        .map(|f| (f.severity, f.matched.clone(), f.suggestion.clone()))
        .collect()
}

#[test]
fn unlinked_existing_file_suggests_a_relative_link() {
    let dir = repo(&["src/main.rs", "docs/guide.md"]);
    let got = check_md(
        dir.path(),
        "docs/guide.md",
        "Start in `src/main.rs` today.\n",
    );
    assert_eq!(
        summary(&got),
        vec![(
            Severity::Block,
            "src/main.rs".into(),
            Some("link it: [`src/main.rs`](../src/main.rs)".into())
        )]
    );
    assert_eq!(got[0].message, "Link the files you name");
    assert_eq!(got[0].rule_id, "link-files");
}

#[test]
fn linked_file_is_fine() {
    let dir = repo(&["src/main.rs"]);
    let src = "See [src/main.rs](src/main.rs) and [`src/main.rs`](src/main.rs).\n";
    assert_eq!(check_md(dir.path(), "README.md", src), vec![]);
}

#[test]
fn named_file_missing_is_a_warn() {
    let dir = repo(&["src/keep.rs", "docs/keep.md"]);
    let got = check_md(
        dir.path(),
        "README.md",
        "Edit `src/gone.rs` and docs/gone.md first.\n",
    );
    assert_eq!(got.len(), 2);
    for f in &got {
        assert_eq!(f.severity, Severity::Warn);
        assert!(f.message.contains("not found"), "{}", f.message);
        assert_eq!(f.suggestion, None);
    }
}

#[test]
fn missing_path_beside_the_markdown_file_warns() {
    let dir = repo(&["guide/notes/keep.md"]);
    let got = check_md(dir.path(), "guide/README.md", "See `notes/gone.md`.\n");
    assert_eq!(
        summary(&got),
        vec![(Severity::Warn, "notes/gone.md".into(), None)]
    );
}

#[test]
fn missing_files_outside_the_repo_stay_quiet() {
    let dir = repo(&["src/keep.rs"]);
    let src = "Read `disc/levels/one.bin`, then `manifest.json` and data/out.csv.\n";
    assert_eq!(check_md(dir.path(), "README.md", src), vec![]);
}

#[test]
fn product_names_and_code_stay_quiet() {
    let dir = repo(&[]);
    let src =
        "Built on Node.js, see `self.size` and `cargo test` and `-v.x` and v1.2.3 and `x.123`.\n";
    assert_eq!(check_md(dir.path(), "README.md", src), vec![]);
}

#[test]
fn bare_name_resolves_by_unique_basename() {
    let dir = repo(&["docs/guide.md"]);
    let got = check_md(dir.path(), "README.md", "Read guide.md before starting.\n");
    assert_eq!(
        summary(&got),
        vec![(
            Severity::Block,
            "guide.md".into(),
            Some("link it: [guide.md](docs/guide.md)".into())
        )]
    );
}

#[test]
fn path_suffix_resolves_anywhere_in_the_repo() {
    let dir = repo(&["crates/core/src/lib.rs"]);
    let got = check_md(dir.path(), "README.md", "Open `src/lib.rs` first.\n");
    assert_eq!(
        got[0].suggestion.as_deref(),
        Some("link it: [`src/lib.rs`](crates/core/src/lib.rs)")
    );
}

#[test]
fn ambiguous_basename_is_unresolved() {
    let dir = repo(&["a/notes.md", "b/notes.md"]);
    let got = check_md(
        dir.path(),
        "README.md",
        "Check `notes.md` and notes.md too.\n",
    );
    assert_eq!(
        summary(&got),
        vec![(
            Severity::Warn,
            "notes.md".into(),
            Some("name it by its path".into())
        )]
    );
    assert_eq!(
        got[0].message,
        "named file `notes.md` matches 2 files, so it can't be linked"
    );
}

#[test]
fn existing_directories_are_left_alone() {
    let dir = repo(&[
        "tools/sdk-1.4/README.md",
        "out/Viewer.app/Info.plist",
        "docs/x.md",
    ]);
    let src = "Use `tools/sdk-1.4/` and `out/Viewer.app` and `sdk-1.4`.\n";
    assert_eq!(check_md(dir.path(), "README.md", src), vec![]);
    let nested = "See `../tools/sdk-1.4` here.\n";
    assert_eq!(check_md(dir.path(), "docs/x.md", nested), vec![]);
}

#[test]
fn broken_relative_link_is_a_warn() {
    let dir = repo(&["docs/real.md"]);
    let src = "See [setup](docs/setup.md), [real](docs/real.md#intro), [web](https://x.io/a.md), [top](#top), [abs](/etc/hosts), [mail](mailto:a@b.c), <a@b.c>.\n";
    let got = check_md(dir.path(), "README.md", src);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].severity, Severity::Warn);
    assert_eq!(got[0].matched, "setup");
    assert!(
        got[0].message.contains("docs/setup.md"),
        "{}",
        got[0].message
    );
}

#[test]
fn percent_encoded_links_resolve() {
    let dir = repo(&["my notes.md"]);
    let got = check_md(
        dir.path(),
        "README.md",
        "See [notes](my%20notes.md) and [bad](%zz.md).\n",
    );
    assert_eq!(summary(&got), vec![(Severity::Warn, "bad".into(), None)]);
}

#[test]
fn suggestion_wraps_paths_with_spaces() {
    let dir = repo(&["docs/my guide.md"]);
    let got = check_md(dir.path(), "README.md", "Read `docs/my guide.md` now.\n");
    assert_eq!(got, vec![], "a path with a space isn't a single mention");
    let dir = repo(&["my docs/x.md"]);
    let got = check_md(dir.path(), "README.md", "Read `x.md` now.\n");
    assert_eq!(
        got[0].suggestion.as_deref(),
        Some("link it: [`x.md`](<my docs/x.md>)")
    );
}

#[test]
fn search_cap_is_reported() {
    let dir = repo(&["a.rs", "b.rs", "src/c.rs"]);
    let rs = ruleset();
    let path = dir.path().join("README.md");
    let resolver = Resolver::new(&path, &rs, 2);
    let spans = extract(&path, "Check `src/nowhere.rs` now.\n", &[]);
    let got = check(&rs.rules[0], &spans[0], &resolver);
    assert_eq!(got.len(), 1);
    assert!(
        got[0].message.contains("search stopped after 2 files"),
        "{}",
        got[0].message
    );
}

#[test]
fn outside_a_repo_only_relative_paths_resolve() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("docs/sub")).unwrap();
    std::fs::write(dir.path().join("docs/sub/a.md"), "x").unwrap();
    std::fs::write(dir.path().join("top.md"), "x").unwrap();
    let got = check_md(
        dir.path(),
        "docs/README.md",
        "See `sub/a.md` and `top.md`.\n",
    );
    assert_eq!(
        summary(&got),
        vec![(
            Severity::Block,
            "sub/a.md".into(),
            Some("link it: [`sub/a.md`](sub/a.md)".into())
        )],
        "`top.md` sits above the markdown file, where nothing searches"
    );
}

#[test]
fn ignored_and_excluded_files_are_not_searched() {
    let dir = repo(&["out/gen.md", "node_modules/pkg/dep.md", "src/real.md"]);
    std::fs::write(dir.path().join(".gitignore"), "out/\n").unwrap();
    let got = check_md(
        dir.path(),
        "README.md",
        "See `gen.md`, `dep.md` and `real.md`.\n",
    );
    assert_eq!(
        summary(&got),
        vec![(
            Severity::Block,
            "real.md".into(),
            Some("link it: [`real.md`](src/real.md)".into())
        )],
        "the ignored and excluded copies never resolve"
    );
}

#[test]
fn plain_text_paths_count_and_links_cover_them() {
    let dir = repo(&["src/main.rs"]);
    let got = check_md(
        dir.path(),
        "README.md",
        "Edit src/main.rs. Or [src/main.rs](src/main.rs).\n",
    );
    assert_eq!(
        summary(&got),
        vec![(
            Severity::Block,
            "src/main.rs".into(),
            Some("link it: [src/main.rs](src/main.rs)".into())
        )]
    );
}

#[test]
fn file_links_only_apply_to_markdown() {
    let dir = repo(&["src/main.rs"]);
    let rs = ruleset();
    let path = dir.path().join("notes.txt");
    let got = evaluate(&rs, &extract(&path, "Edit src/main.rs now.\n", &[]), &path);
    assert_eq!(got, vec![]);
}

#[test]
fn helpers() {
    assert_eq!(percent_decode("a%20b%2Fc%"), "a b/c%");
    assert_eq!(normalize(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
    assert_eq!(normalize(Path::new("./a")), PathBuf::from("a"));
    assert_eq!(
        relative(Path::new("/r/docs"), Path::new("/r/src/x.rs")),
        "../src/x.rs"
    );
    assert_eq!(relative(Path::new("/r"), Path::new("/r/x.rs")), "x.rs");
    assert_eq!(
        local_target("docs/a.md?raw=1"),
        Some(PathBuf::from("docs/a.md"))
    );
    assert_eq!(local_target("~/x.md"), None);
    assert_eq!(local_target(""), None);
}
