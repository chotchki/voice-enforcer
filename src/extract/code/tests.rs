use super::*;
use crate::extract::MASK;

fn spans(lang: Lang, src: &str) -> Vec<(String, Level, usize)> {
    spans_with(lang, src, &[])
}

fn spans_with(lang: Lang, src: &str, directives: &[Regex]) -> Vec<(String, Level, usize)> {
    extract(lang, src, directives)
        .into_iter()
        .map(|s| {
            assert_eq!(s.register, Register::Comment);
            let line = s.first_line();
            (s.text, s.level.expect("comments have a level"), line)
        })
        .collect()
}

fn texts(lang: Lang, src: &str) -> Vec<String> {
    spans(lang, src).into_iter().map(|(t, _, _)| t).collect()
}

const RUST: &str = r#"//! Crate docs mention leverage.
//! Second inner line.

// SPDX-License-Identifier: MIT

use std::fmt; // trailing note

/// Doc for the struct.
/// Still the doc.
#[derive(Debug)]
pub struct Widget {
    /// Field doc.
    size: u8, // trailing field note
}

// Detached comment.

fn run() {
    // first line
    // second line
    let s = "// not a comment leverage";
    /* block
     * continues here
     */
    // eslint-disable-next-line no-console
}

/** Outer block doc. */
fn other() {}
"#;

#[test]
fn rust_comments_and_levels() {
    assert_eq!(
        spans(Lang::Rust, RUST),
        vec![
            (
                "Crate docs mention leverage.\nSecond inner line.".into(),
                Level::Module,
                1
            ),
            ("trailing note".into(), Level::Line, 6),
            ("Doc for the struct.\nStill the doc.".into(), Level::Item, 8),
            ("Field doc.".into(), Level::Item, 12),
            ("trailing field note".into(), Level::Line, 13),
            ("Detached comment.".into(), Level::Line, 16),
            ("first line\nsecond line".into(), Level::Line, 19),
            ("block\ncontinues here".into(), Level::Line, 22),
            ("Outer block doc.".into(), Level::Item, 28),
        ]
    );
}

#[test]
fn rust_line_mapping_inside_merged_block() {
    let spans = extract(Lang::Rust, RUST, &[]);
    let block = spans.iter().find(|s| s.text.starts_with("first")).unwrap();
    assert_eq!(block.line_at(block.text.find("second").unwrap()), 20);
    assert_eq!(block.last_line(), 20);
}

#[test]
fn block_comment_styles() {
    let src = "/*! Inner block doc. */\n\nuse x;\n\n/**/\nfn f() {}\n";
    assert_eq!(
        spans(Lang::Rust, src),
        vec![("Inner block doc.".into(), Level::Module, 1)]
    );
    let src = "/*! Not inner outside Rust. */\nfunc f() {}\n";
    assert_eq!(
        spans(Lang::Swift, src),
        vec![("! Not inner outside Rust.".into(), Level::Item, 1)]
    );
}

#[test]
fn dispatch_reaches_code() {
    let got = crate::extract::extract(std::path::Path::new("src/x.rs"), "// hi\nfn f() {}\n", &[]);
    assert_eq!(got.len(), 1);
}

#[test]
fn doc_and_plain_comments_do_not_merge() {
    let src = "/// doc line\n// plain line\nfn f() {}\n";
    assert_eq!(
        spans(Lang::Rust, src),
        vec![
            ("doc line".into(), Level::Module, 1),
            ("plain line".into(), Level::Item, 2),
        ]
    );
}

#[test]
fn trailing_then_own_line_do_not_merge() {
    let src = "fn f() {\n    let a = 1; // trailing\n    // own line\n    let b = 2;\n}\n";
    assert_eq!(texts(Lang::Rust, src), vec!["trailing", "own line"]);
}

#[test]
fn backtick_code_is_masked() {
    let spans = extract(
        Lang::Rust,
        "// call `leverage()` then stop\nfn f() {}\n",
        &[],
    );
    assert_eq!(spans[0].text, format!("call {MASK} then stop"));
    assert!(matches!(
        &spans[0].annotations[..],
        [Annotation::InlineCode { content, .. }] if content == "leverage()"
    ));
}

#[test]
fn urls_in_comments_are_masked() {
    assert_eq!(
        texts(
            Lang::Rust,
            "// see https://example.com/leverage\nfn f() {}\n"
        ),
        vec![format!("see {MASK}")]
    );
}

#[test]
fn copyright_blocks_are_skipped() {
    let src =
        "/*\n * Copyright 2026 Someone\n * All rights reserved.\n */\n\n// Real header.\nuse x;\n";
    assert_eq!(texts(Lang::Rust, src), vec!["Real header."]);
}

#[test]
fn blank_block_comment_is_dropped() {
    assert_eq!(
        texts(Lang::Rust, "/*\n\n*/\nfn f() {}\n"),
        Vec::<String>::new()
    );
}

#[test]
fn configured_directives_are_skipped() {
    let directive = Regex::new("^lint:allow").unwrap();
    let src = "// lint:allow long-line\n// kept\nfn f() {}\n";
    assert_eq!(
        spans_with(Lang::Rust, src, &[directive]),
        vec![("kept".into(), Level::Item, 2)]
    );
}

const TS: &str = r#"// File header.
import x from "y";

/**
 * Exported function doc.
 */
export function f(): void {
  // inside the body
  const local = 1;
}

// Top-level const doc.
const top = () => 1;

// @ts-ignore
//#region helpers
@dec
class C {
  /** Method doc. */
  m() {}
}

interface I {}
"#;

#[test]
fn typescript_comments_and_levels() {
    assert_eq!(
        spans(Lang::TypeScript, TS),
        vec![
            ("File header.".into(), Level::Module, 1),
            ("Exported function doc.".into(), Level::Item, 5),
            ("inside the body".into(), Level::Line, 8),
            ("Top-level const doc.".into(), Level::Item, 12),
            ("Method doc.".into(), Level::Item, 19),
        ]
    );
}

#[test]
fn tsx_parses() {
    let src = "// Component header.\nexport const A = () => <div>hi</div>;\n";
    assert_eq!(
        spans(Lang::Tsx, src),
        vec![("Component header.".into(), Level::Item, 1)]
    );
}

#[test]
fn javascript_hashbang_and_directives() {
    let src = "#!/usr/bin/env node\n// Header.\n\n/* istanbul ignore next */\nfunction f() {}\n";
    assert_eq!(
        spans(Lang::JavaScript, src),
        vec![("Header.".into(), Level::Module, 2)]
    );
}

const PY: &str = r#"#!/usr/bin/env python3
"""Module docstring mentions leverage."""

import os  # noqa: F401
x = "a string with leverage"  # type: ignore

# Decorated function doc.
@decorator
def f():
    """Function docstring.

    Second paragraph, indented.
    """
    # inside
    return 1


class C:
    r'''Raw class docstring.'''

    def g(self):
        return "not a docstring"
"#;

#[test]
fn python_comments_docstrings_and_levels() {
    assert_eq!(
        spans(Lang::Python, PY),
        vec![
            (
                "Module docstring mentions leverage.".into(),
                Level::Module,
                2
            ),
            ("Decorated function doc.".into(), Level::Item, 7),
            (
                "Function docstring.\n\nSecond paragraph, indented.".into(),
                Level::Item,
                10
            ),
            ("inside".into(), Level::Line, 14),
            ("Raw class docstring.".into(), Level::Item, 19),
        ]
    );
}

#[test]
fn python_docstring_line_mapping() {
    let spans = extract(Lang::Python, PY, &[]);
    let doc = spans
        .iter()
        .find(|s| s.text.starts_with("Function"))
        .unwrap();
    assert_eq!(doc.line_at(doc.text.find("Second").unwrap()), 12);
}

#[test]
fn python_non_docstring_first_statements() {
    let src = "def f():\n    return 1\n\ndef g():\n    x, y = 1, 2\n";
    assert_eq!(texts(Lang::Python, src), Vec::<String>::new());
}

#[test]
fn python_single_quoted_docstring() {
    assert_eq!(
        texts(Lang::Python, "def f():\n    'Short doc.'\n"),
        vec!["Short doc."]
    );
}

const SWIFT: &str = r"// File header.
import Foundation

/// Function doc.
@objc
func f() {
  // inside
  let local = 1
}

/** Class doc. */
class C {
  /// Property doc.
  var size = 0
}

// swiftlint:disable line_length
struct S {}
";

#[test]
fn swift_comments_and_levels() {
    assert_eq!(
        spans(Lang::Swift, SWIFT),
        vec![
            ("File header.".into(), Level::Module, 1),
            ("Function doc.".into(), Level::Item, 4),
            ("inside".into(), Level::Line, 7),
            ("Class doc.".into(), Level::Item, 11),
            ("Property doc.".into(), Level::Item, 13),
        ]
    );
}

const BASH: &str = r##"#!/bin/bash
# Script header.
set -e

# Function doc.
greet() {
  # inside
  echo "# not a comment"
}

# shellcheck disable=SC2086
echo $x
"##;

#[test]
fn bash_comments_and_levels() {
    assert_eq!(
        spans(Lang::Bash, BASH),
        vec![
            ("Script header.".into(), Level::Module, 2),
            ("Function doc.".into(), Level::Item, 5),
            ("inside".into(), Level::Line, 7),
        ]
    );
}

#[test]
fn extensions_map_to_languages() {
    for (ext, lang) in [
        ("rs", Lang::Rust),
        ("swift", Lang::Swift),
        ("ts", Lang::TypeScript),
        ("mts", Lang::TypeScript),
        ("tsx", Lang::Tsx),
        ("js", Lang::JavaScript),
        ("jsx", Lang::JavaScript),
        ("mjs", Lang::JavaScript),
        ("cjs", Lang::JavaScript),
        ("py", Lang::Python),
        ("sh", Lang::Bash),
        ("bash", Lang::Bash),
        ("zsh", Lang::Bash),
    ] {
        assert_eq!(Lang::from_extension(ext), Some(lang), "{ext}");
    }
    assert_eq!(Lang::from_extension("go"), None);
}

#[test]
fn javascript_levels() {
    let src = "// Header.\n\n// Function doc.\nfunction f() {\n  // inside\n  return 1;\n}\n\n// Detached.\n\nfunction g() {}\n";
    assert_eq!(
        spans(Lang::JavaScript, src),
        vec![
            ("Header.".into(), Level::Module, 1),
            ("Function doc.".into(), Level::Item, 3),
            ("inside".into(), Level::Line, 5),
            ("Detached.".into(), Level::Line, 9),
        ]
    );
}

#[test]
fn typescript_decorated_class() {
    let src = "import x from 'y';\n// Decorated class doc.\n@dec\nclass C {}\n";
    assert_eq!(
        spans(Lang::TypeScript, src),
        vec![("Decorated class doc.".into(), Level::Item, 2)]
    );
}

#[test]
fn python_header_comment_and_blank_line() {
    let src = "#!/usr/bin/env python3\n# Header comment.\nimport os\n\n# Detached.\n\ndef f():\n    pass\n";
    assert_eq!(
        spans(Lang::Python, src),
        vec![
            ("Header comment.".into(), Level::Module, 2),
            ("Detached.".into(), Level::Line, 5),
        ]
    );
}
