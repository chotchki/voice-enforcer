//! Turns a verdict into findings, keeping only quotes that really are in the judged text.

use crate::extract::ProseSpan;
use crate::judges::input::Input;
use crate::judges::runner::Verdict;
use crate::rules::Finding;
use crate::ruleset::Judge;

const MAX_MATCHED_CHARS: usize = 100;

pub(crate) fn findings(
    judge: &Judge,
    verdict: &Verdict,
    input: &Input,
    spans: &[ProseSpan],
) -> Vec<Finding> {
    verdict
        .findings
        .iter()
        .filter_map(|raw| {
            let (line, end_line) = input
                .judged
                .iter()
                .find_map(|&i| locate(&spans[i], &raw.quote))?;
            Some(Finding {
                rule_id: judge.id.clone(),
                severity: judge.severity,
                line,
                end_line,
                matched: shorten(&normalize(&raw.quote).0),
                message: raw.explanation.trim().to_owned(),
                suggestion: raw
                    .suggestion
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            })
        })
        .collect()
}

/// The first and last line of `quote` in the span's raw text, comparing with whitespace runs
/// collapsed, then again ignoring ASCII case. `None` when it isn't there (a hallucinated quote).
fn locate(span: &ProseSpan, quote: &str) -> Option<(usize, usize)> {
    let (needle, _) = normalize(quote);
    if needle.is_empty() {
        return None;
    }
    let (hay, offsets) = normalize(&span.raw);
    let at = hay
        .find(&needle)
        .or_else(|| hay.to_ascii_lowercase().find(&needle.to_ascii_lowercase()))?;
    let start = offsets[at];
    let end = offsets[at + needle.len() - 1];
    Some((span.raw_line_at(start), span.raw_line_at(end)))
}

/// `text` with every whitespace run collapsed to one space and trimmed, plus the original byte
/// offset of each byte in the result.
fn normalize(text: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(text.len());
    let mut offsets = Vec::with_capacity(text.len());
    let mut pending_space: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if !out.is_empty() && pending_space.is_none() {
                pending_space = Some(i);
            }
            continue;
        }
        if let Some(at) = pending_space.take() {
            out.push(' ');
            offsets.push(at);
        }
        out.push(c);
        offsets.extend(std::iter::repeat_n(i, c.len_utf8()));
    }
    (out, offsets)
}

fn shorten(text: &str) -> String {
    match text.char_indices().nth(MAX_MATCHED_CHARS) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::extract::extract;
    use crate::judges::runner::RawFinding;
    use crate::ruleset::Scope;
    use crate::types::{Register, Severity};

    fn judge() -> Judge {
        Judge {
            id: "no-recap".into(),
            severity: Severity::Block,
            scope: Scope::Diff,
            registers: vec![Register::LongForm, Register::Comment],
            rubric: "r".into(),
        }
    }

    fn verdict(quotes: &[&str]) -> Verdict {
        Verdict {
            findings: quotes
                .iter()
                .map(|q| RawFinding {
                    quote: (*q).to_owned(),
                    explanation: " A recap. ".into(),
                    suggestion: Some(" ".into()),
                })
                .collect(),
        }
    }

    fn run(path: &str, src: &str, judged: Vec<usize>, quotes: &[&str]) -> Vec<Finding> {
        let spans = extract(Path::new(path), src, &[]);
        let input = Input {
            text: String::new(),
            judged,
        };
        findings(&judge(), &verdict(quotes), &input, &spans)
    }

    #[test]
    fn real_quote_becomes_a_finding() {
        let got = run(
            "a.md",
            "Intro.\n\nIt works. In conclusion, it works.\n",
            vec![1],
            &["In conclusion, it works."],
        );
        assert_eq!(
            got,
            vec![Finding {
                rule_id: "no-recap".into(),
                severity: Severity::Block,
                line: 3,
                end_line: 3,
                matched: "In conclusion, it works.".into(),
                message: "A recap.".into(),
                suggestion: None,
            }]
        );
    }

    #[test]
    fn hallucinated_quote_is_dropped() {
        assert_eq!(
            run("a.md", "Plain text.\n", vec![0], &["Never written."]),
            vec![]
        );
        assert_eq!(run("a.md", "Plain text.\n", vec![0], &["   "]), vec![]);
    }

    #[test]
    fn wrapped_quote_reports_first_and_last_line() {
        let got = run(
            "a.md",
            "One more time,\nto sum it all up.\n",
            vec![0],
            &["time, to sum it"],
        );
        assert_eq!((got[0].line, got[0].end_line), (1, 2));
    }

    #[test]
    fn case_differences_still_match() {
        let got = run(
            "a.md",
            "In Conclusion it works.\n",
            vec![0],
            &["in conclusion it works."],
        );
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn only_judged_spans_count() {
        let src = "Old paragraph says In conclusion.\n\nNew paragraph.\n";
        assert_eq!(run("a.md", src, vec![1], &["In conclusion."]), vec![]);
    }

    #[test]
    fn code_context_is_never_a_source() {
        let src = "fn f() {\n    // bump it\n    count += 1;\n}\n";
        assert_eq!(run("x.rs", src, vec![0], &["count += 1;"]), vec![]);
        assert_eq!(run("x.rs", src, vec![0], &["bump it"]).len(), 1);
    }

    #[test]
    fn raw_text_includes_inline_code() {
        let got = run(
            "a.md",
            "Run `cargo test` to see.\n",
            vec![0],
            &["Run `cargo test`"],
        );
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn suggestions_are_trimmed_and_long_quotes_shortened() {
        let long = "word ".repeat(40);
        let spans = extract(Path::new("a.md"), &format!("{long}\n"), &[]);
        let input = Input {
            text: String::new(),
            judged: vec![0],
        };
        let v = Verdict {
            findings: vec![RawFinding {
                quote: long.clone(),
                explanation: "e".into(),
                suggestion: Some(" cut it ".into()),
            }],
        };
        let got = findings(&judge(), &v, &input, &spans);
        assert_eq!(got[0].suggestion.as_deref(), Some("cut it"));
        assert!(got[0].matched.ends_with('…'));
        assert_eq!(got[0].matched.chars().count(), MAX_MATCHED_CHARS + 1);
    }

    #[test]
    fn normalize_maps_offsets() {
        let (text, offsets) = normalize("  a \n\t b c ");
        assert_eq!(text, "a b c");
        assert_eq!(offsets, vec![2, 3, 7, 8, 9]);
        let (text, offsets) = normalize("é x");
        assert_eq!(text, "é x");
        assert_eq!(offsets, vec![0, 0, 2, 3]);
    }
}
