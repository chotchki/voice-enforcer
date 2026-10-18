//! Text already committed at git HEAD didn't come from this write, wherever it lands now.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use wait_timeout::ChildExt;

use crate::extract::ProseSpan;
use crate::rules::Finding;

/// Past these the remaining findings are reported as they would be without the check.
const MAX_SPANS: usize = 50;
const BUDGET: Duration = Duration::from_secs(2);
const MAX_CANDIDATES: usize = 200;
/// A shorter search line matches half the repo and settles nothing.
const MIN_NEEDLE: usize = 8;

pub(super) struct Head<'a> {
    git: &'a OsStr,
    root: Option<PathBuf>,
    after: &'a str,
    spans: &'a [ProseSpan],
    known: HashMap<usize, bool>,
    deadline: Instant,
}

impl<'a> Head<'a> {
    pub(super) fn new(git: &'a OsStr, path: &Path, after: &'a str, spans: &'a [ProseSpan]) -> Self {
        let root = path
            .parent()
            .and_then(|dir| dir.ancestors().find(|d| d.join(".git").exists()))
            .map(Path::to_path_buf);
        Self {
            git,
            root,
            after,
            spans,
            known: HashMap::new(),
            deadline: Instant::now() + BUDGET,
        }
    }

    /// `findings` minus the ones inside a span whose text is already at HEAD, keeping any that
    /// `exempt` picks.
    pub(super) fn drop_existing(
        &mut self,
        findings: Vec<Finding>,
        exempt: impl Fn(&Finding) -> bool,
    ) -> Vec<Finding> {
        let mut wanted: Vec<usize> = findings
            .iter()
            .filter(|f| !exempt(f) && !self.known.contains_key(&f.span))
            .map(|f| f.span)
            .collect();
        wanted.sort_unstable();
        wanted.dedup();
        wanted.truncate(MAX_SPANS.saturating_sub(self.known.len()));
        if !wanted.is_empty() {
            self.look_up(&wanted);
        }
        findings
            .into_iter()
            .filter(|f| exempt(f) || !self.known.get(&f.span).copied().unwrap_or(false))
            .collect()
    }

    fn look_up(&mut self, spans: &[usize]) {
        for &s in spans {
            self.known.insert(s, false);
        }
        let Some(root) = self.root.clone() else {
            return;
        };
        let texts: Vec<(usize, String, &str)> = spans
            .iter()
            .filter_map(|&s| {
                let span = self.spans.get(s)?;
                let lines: Vec<&str> = self
                    .after
                    .lines()
                    .skip(span.first_line() - 1)
                    .take(span.last_line() - span.first_line() + 1)
                    .collect();
                let needle = lines.iter().map(|l| l.trim()).max_by_key(|l| l.len())?;
                (needle.len() >= MIN_NEEDLE).then(|| (s, collapse(&lines.join("\n")), needle))
            })
            .collect();
        if texts.is_empty() {
            return;
        }

        let mut grep: Vec<&str> = vec!["grep", "-F", "-I", "-l", "-z"];
        for (_, _, needle) in &texts {
            grep.extend(["-e", needle]);
        }
        grep.extend(["HEAD", "--"]);
        let Some(listed) = self.git(&root, &grep, None, &[0, 1]) else {
            return;
        };
        let names: Vec<&[u8]> = listed
            .split(|&b| b == 0)
            .filter(|n| !n.is_empty())
            .take(MAX_CANDIDATES)
            .collect();
        if names.is_empty() {
            return;
        }
        let mut request = names.join(&b'\n');
        request.push(b'\n');
        let Some(batch) = self.git(&root, &["cat-file", "--batch"], Some(&request), &[0]) else {
            return;
        };
        for blob in blobs(&batch) {
            let blob = collapse(&String::from_utf8_lossy(blob));
            for (s, text, _) in &texts {
                if blob.contains(text.as_str()) {
                    self.known.insert(*s, true);
                }
            }
        }
    }

    /// Stdout when git exits with one of `ok` before the budget runs out, otherwise `None`.
    fn git(&self, root: &Path, args: &[&str], input: Option<&[u8]>, ok: &[i32]) -> Option<Vec<u8>> {
        let left = self.deadline.checked_duration_since(Instant::now())?;
        let mut child = Command::new(self.git)
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let stdin = child.stdin.take();
        let mut stdout = child.stdout.take()?;
        let (status, out) = std::thread::scope(|s| {
            s.spawn(move || {
                if let (Some(mut stdin), Some(input)) = (stdin, input) {
                    let _ = stdin.write_all(input);
                }
            });
            let out = s.spawn(move || {
                let mut buf = Vec::new();
                let _ = stdout.read_to_end(&mut buf);
                buf
            });
            let status = child.wait_timeout(left).ok().flatten();
            if status.is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
            (status, out.join().unwrap_or_default())
        });
        status
            .and_then(|s| s.code())
            .filter(|c| ok.contains(c))
            .map(|_| out)
    }
}

/// The contents in `git cat-file --batch` output, skipping `missing` entries.
fn blobs(batch: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut rest = batch;
    while let Some(end) = rest.iter().position(|&b| b == b'\n') {
        let header = String::from_utf8_lossy(&rest[..end]);
        rest = &rest[end + 1..];
        let Some(size) = header
            .rsplit(' ')
            .next()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|_| !header.ends_with(" missing"))
        else {
            continue;
        };
        let Some(content) = rest.get(..size) else {
            break;
        };
        out.push(content);
        rest = rest.get(size + 1..).unwrap_or_default();
    }
    out
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::extract;
    use crate::types::Severity;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    /// A repo whose HEAD holds `files`.
    fn repo(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        for (name, content) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        git(dir.path(), &["add", "-A"]);
        git(
            dir.path(),
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
        );
        dir
    }

    fn finding(span: usize, line: usize) -> Finding {
        Finding {
            rule_id: "r".into(),
            severity: Severity::Block,
            line,
            end_line: line,
            matched: "x".into(),
            message: "m".into(),
            suggestion: None,
            span,
        }
    }

    const OLD: &str =
        "fn main() {\n        // Leverage the pool so every worker shares it.\n        run();\n}\n";

    #[test]
    fn moved_comment_is_dropped_and_new_one_kept() {
        let dir = repo(&[("src/main.rs", OLD)]);
        let after = "pub fn boot() {\n    // Leverage the pool so every worker shares it.\n    run();\n    // Leverage a brand new cache for the boot path.\n}\n";
        let path = dir.path().join("src/boot.rs");
        let spans = extract(&path, after, &[]);
        let mut head = Head::new(OsStr::new("git"), &path, after, &spans);
        let kept = head.drop_existing(vec![finding(0, 2), finding(1, 4)], |_| false);
        assert_eq!(kept, vec![finding(1, 4)]);
    }

    #[test]
    fn a_changed_word_makes_it_new_text() {
        let dir = repo(&[("src/main.rs", OLD)]);
        let after = "pub fn boot() {\n    // Leverage the pool so each worker shares it.\n}\n";
        let path = dir.path().join("src/boot.rs");
        let spans = extract(&path, after, &[]);
        let mut head = Head::new(OsStr::new("git"), &path, after, &spans);
        assert_eq!(head.drop_existing(vec![finding(0, 2)], |_| false).len(), 1);
    }

    #[test]
    fn exempt_findings_are_kept_and_never_looked_up() {
        let dir = repo(&[("src/main.rs", OLD)]);
        let after = "// Leverage the pool so every worker shares it.\n";
        let path = dir.path().join("src/boot.rs");
        let spans = extract(&path, after, &[]);
        let mut head = Head::new(OsStr::new("git"), &path, after, &spans);
        assert_eq!(head.drop_existing(vec![finding(0, 1)], |_| true).len(), 1);
        assert!(head.known.is_empty());
        assert_eq!(head.drop_existing(vec![finding(0, 1)], |_| false).len(), 0);
    }

    #[test]
    fn worktree_checkout_works_too() {
        let dir = repo(&[("src/main.rs", OLD)]);
        let wt = dir.path().join("wt");
        git(dir.path(), &["worktree", "add", "-q", wt.to_str().unwrap()]);
        assert!(wt.join(".git").is_file(), "a worktree's .git is a file");
        let after = "// Leverage the pool so every worker shares it.\n";
        let path = wt.join("src/boot.rs");
        let spans = extract(&path, after, &[]);
        let mut head = Head::new(OsStr::new("git"), &path, after, &spans);
        assert_eq!(head.drop_existing(vec![finding(0, 1)], |_| false).len(), 0);
    }

    #[test]
    fn no_git_no_repo_or_short_text_keeps_everything() {
        let dir = repo(&[("src/main.rs", OLD)]);
        let after = "// Leverage the pool so every worker shares it.\n\n// Hi.\n";
        let path = dir.path().join("src/boot.rs");
        let spans = extract(&path, after, &[]);
        let bogus = OsStr::new("/nonexistent/git");
        let mut head = Head::new(bogus, &path, after, &spans);
        assert_eq!(head.drop_existing(vec![finding(0, 1)], |_| false).len(), 1);

        let elsewhere = tempfile::tempdir().unwrap();
        let path = elsewhere.path().join("boot.rs");
        let mut head = Head::new(OsStr::new("git"), &path, after, &spans);
        assert_eq!(head.drop_existing(vec![finding(0, 1)], |_| false).len(), 1);

        let path = dir.path().join("src/boot.rs");
        let mut head = Head::new(OsStr::new("git"), &path, after, &spans);
        assert_eq!(
            head.drop_existing(vec![finding(1, 3)], |_| false).len(),
            1,
            "too short to search for"
        );
        assert_eq!(
            head.drop_existing(vec![finding(9, 9)], |_| false).len(),
            1,
            "a span index past the end"
        );
    }

    #[test]
    fn nothing_matches_or_budget_spent() {
        let dir = repo(&[("src/main.rs", OLD)]);
        let after = "// An entirely different comment, long enough.\n";
        let path = dir.path().join("src/boot.rs");
        let spans = extract(&path, after, &[]);
        let mut head = Head::new(OsStr::new("git"), &path, after, &spans);
        assert_eq!(head.drop_existing(vec![finding(0, 1)], |_| false).len(), 1);

        let after = "// Leverage the pool so every worker shares it.\n";
        let spans = extract(&path, after, &[]);
        let mut head = Head::new(OsStr::new("git"), &path, after, &spans);
        head.deadline = Instant::now();
        assert_eq!(head.drop_existing(vec![finding(0, 1)], |_| false).len(), 1);
    }

    /// A git whose search always finds a candidate, so a test can force how the read fails.
    #[cfg(unix)]
    fn fake_git(dir: &Path, cat_file: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("fake-git");
        let script = format!(
            "#!/bin/sh\ncase \"$3\" in\n  grep) printf 'HEAD:src/main.rs\\0' ;;\n  cat-file) {cat_file} ;;\nesac\n"
        );
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_or_hanging_git_keeps_the_findings() {
        let dir = repo(&[("src/main.rs", OLD)]);
        let after = "// Leverage the pool so every worker shares it.\n";
        let path = dir.path().join("src/boot.rs");
        let spans = extract(&path, after, &[]);

        let failing = fake_git(dir.path(), "exit 1");
        let mut head = Head::new(failing.as_os_str(), &path, after, &spans);
        assert_eq!(head.drop_existing(vec![finding(0, 1)], |_| false).len(), 1);

        let hanging = fake_git(dir.path(), "exec sleep 5");
        let mut head = Head::new(hanging.as_os_str(), &path, after, &spans);
        head.deadline = Instant::now() + Duration::from_millis(300);
        let started = Instant::now();
        assert_eq!(head.drop_existing(vec![finding(0, 1)], |_| false).len(), 1);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "killed at the budget"
        );
    }

    #[test]
    fn batch_parsing() {
        let batch = b"abc blob 3\nxyz\nHEAD:gone missing\ndef blob 2\nhi\n";
        assert_eq!(blobs(batch), vec![b"xyz".as_slice(), b"hi".as_slice()]);
        assert_eq!(blobs(b"abc blob 9\nshort"), Vec::<&[u8]>::new());
        assert_eq!(blobs(b"garbage\n"), Vec::<&[u8]>::new());
        assert_eq!(collapse("  a \n\t b "), "a b");
    }
}
