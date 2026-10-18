//! The one path from file text to findings, shared by `hook` and `check`.

use std::path::Path;

use crate::extract::{self, ProseSpan};
use crate::rules::{self, Finding};
use crate::ruleset::Ruleset;

#[derive(Debug, Default)]
pub struct Evaluated {
    pub spans: Vec<ProseSpan>,
    pub findings: Vec<Finding>,
}

/// The prose spans and deterministic findings for `text` as the content of `path` (absolute),
/// or nothing when the path is excluded.
pub fn evaluate(ruleset: &Ruleset, path: &Path, text: &str) -> Evaluated {
    if ruleset.is_excluded(path) {
        return Evaluated::default();
    }
    let spans = extract::extract(path, text, &ruleset.comment_directives);
    let findings = rules::evaluate(ruleset, &spans, path);
    Evaluated { spans, findings }
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
            evaluate(&rs, Path::new("/r/node_modules/x/README.md"), "leverage\n").findings,
            vec![]
        );
        assert_eq!(
            evaluate(&rs, Path::new("/r/README.md"), "leverage\n")
                .findings
                .len(),
            1
        );
    }
}
