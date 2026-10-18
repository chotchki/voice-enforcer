//! The text a judge reads: labeled spans, the changed ones marked, code context set apart.

use std::collections::HashSet;
use std::fmt::Write as _;

use crate::extract::ProseSpan;
use crate::ruleset::{Judge, Scope};
use crate::types::Register;

/// Past this a call is slow, costly and likely wrong, so the judge is skipped with a note.
pub(crate) const MAX_INPUT_CHARS: usize = 200_000;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Prepared {
    /// No changed prose in this judge's registers: don't call it.
    Skip,
    TooLarge(usize),
    Ready(Input),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Input {
    pub text: String,
    /// Indices into the spans slice of the spans a quote may come from (the changed ones).
    pub judged: Vec<usize>,
}

/// Whether any line of `span` changed; `None` means every line did (a new file, or `check`).
pub(crate) fn is_changed(span: &ProseSpan, changed: Option<&HashSet<usize>>) -> bool {
    changed.is_none_or(|c| (span.first_line()..=span.last_line()).any(|l| c.contains(&l)))
}

pub(crate) fn prepare(
    judge: &Judge,
    spans: &[ProseSpan],
    changed: Option<&HashSet<usize>>,
) -> Prepared {
    let candidates: Vec<(usize, &ProseSpan, bool)> = spans
        .iter()
        .enumerate()
        .filter(|(_, s)| judge.applies_to(s.register))
        .map(|(i, s)| (i, s, is_changed(s, changed)))
        .collect();
    let judged: Vec<usize> = candidates
        .iter()
        .filter(|(_, _, c)| *c)
        .map(|(i, _, _)| *i)
        .collect();
    if judged.is_empty() {
        return Prepared::Skip;
    }

    let mut text = String::new();
    let mut n = 0;
    for (_, span, is_changed) in candidates {
        if judge.scope == Scope::Diff && !is_changed {
            continue;
        }
        n += 1;
        let lines = match (span.first_line(), span.last_line()) {
            (a, b) if a == b => format!("line {a}"),
            (a, b) => format!("lines {a}-{b}"),
        };
        let kind = match (span.register, span.level) {
            (Register::LongForm, _) => "long_form".to_owned(),
            (Register::Comment, Some(level)) => format!("comment ({})", level.as_str()),
            (Register::Comment, None) => "comment".to_owned(),
        };
        let mark = if is_changed { " · CHANGED" } else { "" };
        let _ = writeln!(text, "=== span {n} · {lines} · {kind}{mark} ===");
        let _ = writeln!(text, "{}", span.raw);
        if let Some(context) = &span.code_context {
            let _ = writeln!(text, "--- code context (not judged) ---");
            let _ = writeln!(text, "{context}");
        }
    }

    let chars = text.chars().count();
    if chars > MAX_INPUT_CHARS {
        return Prepared::TooLarge(chars);
    }
    Prepared::Ready(Input { text, judged })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::extract::extract;
    use crate::types::Severity;

    fn judge(scope: Scope, registers: Vec<Register>) -> Judge {
        Judge {
            id: "j".into(),
            severity: Severity::Warn,
            scope,
            registers,
            rubric: "r".into(),
        }
    }

    const README: &str = "# Title\n\nFirst paragraph stays.\n\nSecond paragraph `changed` here\nand wraps.\n\nThird stays.\n";

    impl Prepared {
        fn ready(self) -> Option<Input> {
            if let Prepared::Ready(input) = self {
                Some(input)
            } else {
                None
            }
        }
    }

    fn ready(p: Prepared) -> Input {
        p.ready().expect("a judge input")
    }

    #[test]
    fn diff_scope_sends_only_changed_spans() {
        let spans = extract(Path::new("README.md"), README, &[]);
        let changed: HashSet<usize> = [6].into();
        let input = ready(prepare(
            &judge(Scope::Diff, vec![Register::LongForm]),
            &spans,
            Some(&changed),
        ));
        assert_eq!(input.judged, vec![2]);
        insta::assert_snapshot!("diff_input", input.text);
    }

    #[test]
    fn whole_scope_marks_the_changed_spans() {
        let spans = extract(Path::new("README.md"), README, &[]);
        let changed: HashSet<usize> = [5].into();
        let input = ready(prepare(
            &judge(Scope::Whole, vec![Register::LongForm]),
            &spans,
            Some(&changed),
        ));
        assert_eq!(input.judged, vec![2]);
        insta::assert_snapshot!("whole_input", input.text);
    }

    #[test]
    fn comment_comes_with_code_context() {
        let src = "fn f() {\n    // increment the counter\n    count += 1;\n}\n";
        let spans = extract(Path::new("x.rs"), src, &[]);
        let input = ready(prepare(
            &judge(Scope::Diff, vec![Register::Comment]),
            &spans,
            None,
        ));
        insta::assert_snapshot!("comment_input", input.text);
    }

    #[test]
    fn nothing_changed_in_the_judges_registers_skips() {
        let spans = extract(Path::new("README.md"), README, &[]);
        let changed: HashSet<usize> = [99].into();
        assert_eq!(
            prepare(
                &judge(Scope::Whole, vec![Register::LongForm]),
                &spans,
                Some(&changed)
            ),
            Prepared::Skip
        );
        assert_eq!(
            prepare(&judge(Scope::Diff, vec![Register::Comment]), &spans, None),
            Prepared::Skip,
            "a comment-only judge never sees markdown"
        );
        assert_eq!(
            prepare(&judge(Scope::Diff, vec![Register::Comment]), &spans, None).ready(),
            None
        );
    }

    #[test]
    fn oversized_input_is_refused() {
        let big = format!("{}\n", "word ".repeat(MAX_INPUT_CHARS / 4));
        let spans = extract(Path::new("big.md"), &big, &[]);
        assert!(matches!(
            prepare(&judge(Scope::Diff, vec![Register::LongForm]), &spans, None),
            Prepared::TooLarge(n) if n > MAX_INPUT_CHARS
        ));
    }

    #[test]
    fn comment_without_a_level_is_labeled_plainly() {
        let spans = extract(Path::new("x.rs"), "// note\n", &[]);
        let mut span = spans[0].clone();
        span.level = None;
        let input = ready(prepare(
            &judge(Scope::Diff, vec![Register::Comment]),
            &[span],
            None,
        ));
        assert!(
            input
                .text
                .starts_with("=== span 1 · line 1 · comment · CHANGED ==="),
            "{}",
            input.text
        );
    }
}
