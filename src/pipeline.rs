//! The one path from file text to findings, shared by `hook` and `check`.

use std::path::Path;

use crate::extract;
use crate::rules::{self, Finding};
use crate::ruleset::Ruleset;

/// Findings for `text` as the content of `path` (absolute), or none when the path is excluded.
pub fn findings(ruleset: &Ruleset, path: &Path, text: &str) -> Vec<Finding> {
    if ruleset.is_excluded(path) {
        return Vec::new();
    }
    let spans = extract::extract(path, text, &ruleset.comment_directives);
    rules::evaluate(ruleset, &spans, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ruleset::parse;

    #[test]
    fn excluded_paths_yield_nothing() {
        let rs = parse(
            "version = 1\n[[rule]]\nid = \"p\"\nkind = \"phrase\"\nseverity = \"block\"\nmessage = \"m\"\nphrases = [\"leverage\"]\n",
            Path::new("r.toml"),
            Path::new("/h"),
        )
        .unwrap();
        assert_eq!(
            findings(&rs, Path::new("/r/node_modules/x/README.md"), "leverage\n"),
            vec![]
        );
        assert_eq!(
            findings(&rs, Path::new("/r/README.md"), "leverage\n").len(),
            1
        );
    }
}
