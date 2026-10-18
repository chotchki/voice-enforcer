use std::path::{Path, PathBuf};

use crate::ruleset::Ruleset;

/// Files under `root`, skipping git-ignored and excluded paths. Excluded directories are pruned
/// whole rather than walked and filtered.
pub(crate) fn files(root: &Path, ruleset: &Ruleset) -> impl Iterator<Item = PathBuf> {
    let excludes = ruleset.exclude_set();
    ignore::WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .filter_entry(move |e| {
            let probe = if e.file_type().is_some_and(|t| t.is_dir()) {
                e.path().join("x")
            } else {
                e.path().to_path_buf()
            };
            !excludes.is_match(&probe)
        })
        .build()
        .flatten()
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(ignore::DirEntry::into_path)
}
