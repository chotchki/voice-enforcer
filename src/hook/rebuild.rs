//! Rebuilds a file as it will read after Write, Edit or MultiEdit runs.

use std::io::ErrorKind;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct WriteInput {
    pub file_path: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct EditInput {
    pub file_path: String,
    #[serde(flatten)]
    pub edit: EditOp,
}

#[derive(Debug, Deserialize)]
pub(super) struct MultiEditInput {
    pub file_path: String,
    pub edits: Vec<EditOp>,
}

#[derive(Debug, Deserialize)]
pub(super) struct EditOp {
    pub old_string: String,
    pub new_string: String,
    #[serde(default)]
    pub replace_all: bool,
}

/// The file before and after the tool runs; `before` is `None` for a file that doesn't exist yet.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Rebuilt {
    pub before: Option<String>,
    pub after: String,
}

/// `Ok(None)` for a missing file, `Err` when it exists but can't be read as UTF-8 text.
fn read(path: &Path) -> Result<Option<String>, ()> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(_) => Err(()),
    }
}

pub(super) fn write(path: &Path, content: String) -> Option<Rebuilt> {
    let before = read(path).ok()?;
    Some(Rebuilt {
        before,
        after: content,
    })
}

pub(super) fn edit(path: &Path, edits: &[EditOp]) -> Option<Rebuilt> {
    let before = read(path).ok()?;
    let mut text = before.clone();
    for op in edits {
        text = Some(apply(text, op)?);
    }
    Some(Rebuilt {
        before,
        after: text?,
    })
}

/// Edit's semantics: an empty `old_string` creates a missing file, otherwise the match must be
/// unique unless `replace_all` is set. `None` when the real tool would refuse.
fn apply(text: Option<String>, op: &EditOp) -> Option<String> {
    match text {
        None if op.old_string.is_empty() => Some(op.new_string.clone()),
        None => None,
        Some(_) if op.old_string.is_empty() => None,
        Some(t) => {
            let count = t.matches(&op.old_string).count();
            match (count, op.replace_all) {
                (0, _) => None,
                (_, true) => Some(t.replace(&op.old_string, &op.new_string)),
                (1, false) => Some(t.replacen(&op.old_string, &op.new_string, 1)),
                _ => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(old: &str, new: &str, all: bool) -> EditOp {
        EditOp {
            old_string: old.into(),
            new_string: new.into(),
            replace_all: all,
        }
    }

    fn file(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.md");
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn write_new_and_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.md");
        assert_eq!(
            write(&path, "hi".into()),
            Some(Rebuilt {
                before: None,
                after: "hi".into()
            })
        );
        std::fs::write(&path, "old").unwrap();
        assert_eq!(
            write(&path, "hi".into()).unwrap().before.as_deref(),
            Some("old")
        );
    }

    #[test]
    fn unreadable_file_passes_through() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            write(dir.path(), "x".into()),
            None,
            "a directory isn't a file"
        );
        let path = dir.path().join("bin.md");
        std::fs::write(&path, [0xff, 0xfe]).unwrap();
        assert_eq!(edit(&path, &[op("a", "b", false)]), None);
    }

    #[test]
    fn edit_replaces_the_unique_match() {
        let (_d, path) = file("the cache is fast");
        let got = edit(&path, &[op("the cache", "the shiny new cache", false)]).unwrap();
        assert_eq!(got.after, "the shiny new cache is fast");
        assert_eq!(got.before.as_deref(), Some("the cache is fast"));
    }

    #[test]
    fn ambiguous_or_missing_match_passes_through() {
        let (_d, path) = file("a a");
        assert_eq!(edit(&path, &[op("a", "b", false)]), None);
        assert_eq!(edit(&path, &[op("zzz", "b", false)]), None);
        assert_eq!(edit(&path, &[op("a", "b", true)]).unwrap().after, "b b");
    }

    #[test]
    fn multi_edit_applies_in_order() {
        let (_d, path) = file("one");
        let got = edit(&path, &[op("one", "two", false), op("two", "three", false)]).unwrap();
        assert_eq!(got.after, "three");
        assert_eq!(
            edit(&path, &[op("one", "two", false), op("one", "x", false)]),
            None
        );
    }

    #[test]
    fn empty_old_string_creates_a_missing_file_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.md");
        let got = edit(&path, &[op("", "fresh", false)]).unwrap();
        assert_eq!((got.before, got.after.as_str()), (None, "fresh"));
        assert_eq!(edit(&path, &[op("x", "y", false)]), None);
        let (_d, existing) = file("content");
        assert_eq!(edit(&existing, &[op("", "y", false)]), None);
    }
}
