//! Sentence boundaries for prose that may start sentences in lowercase (so capitalization is never
//! a signal): `.`, `!` or `?` followed by whitespace or the end of the text, minus abbreviations.

use std::ops::Range;

use crate::extract::MASK;

const ABBREVIATIONS: &[&str] = &[
    "e.g", "i.e", "etc", "vs", "cf", "al", "approx", "fig", "eq", "mr", "mrs", "ms", "dr", "jr",
    "sr", "inc", "ltd", "viz", "resp",
];

const CLOSERS: &[char] = &['"', '\'', ')', ']', '\u{201D}', '\u{2019}'];

/// Trimmed byte ranges of each sentence; runs with no letter, digit or masked code are dropped.
pub fn sentences(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if !matches!(c, '.' | '!' | '?') {
            continue;
        }
        let mut end = i + c.len_utf8();
        while let Some(&(j, next)) = chars.peek() {
            if matches!(next, '.' | '!' | '?') || CLOSERS.contains(&next) {
                end = j + next.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        let at_break = text[end..].chars().next().is_none_or(char::is_whitespace);
        if at_break && !(c == '.' && is_abbreviation(text, i)) && !is_soft_stop(&text[i..end]) {
            push_trimmed(&mut out, text, start..end);
            start = end;
        }
    }
    push_trimmed(&mut out, text, start..text.len());
    out
}

pub fn count(text: &str) -> usize {
    sentences(text).len()
}

/// The sentence holding byte `offset` (or the next one after it), falling back to all of `text`.
pub fn containing(text: &str, offset: usize) -> Range<usize> {
    sentences(text)
        .into_iter()
        .find(|r| offset < r.end)
        .unwrap_or(0..text.len())
}

/// An ellipsis, or a `?` / `!` inside a quote (`"Retry?" after 5s`): both usually run on, and
/// undercounting is the safe side of a limit.
fn is_soft_stop(run: &str) -> bool {
    const QUOTES: &[char] = &['"', '\'', '\u{201D}', '\u{2019}'];
    let mut chars = run.chars().peekable();
    let mut dots = 0;
    while let Some(c) = chars.next() {
        match c {
            '.' => dots += 1,
            '?' | '!' if chars.peek().is_some_and(|n| QUOTES.contains(n)) => return true,
            _ => {}
        }
    }
    dots >= 2
}

fn is_abbreviation(text: &str, dot: usize) -> bool {
    let before = &text[..dot];
    let word_start = before.rfind(char::is_whitespace).map_or(0, |i| {
        i + before[i..].chars().next().map_or(1, char::len_utf8)
    });
    let word = before[word_start..].trim_start_matches(['(', '"', '\'', '[']);
    let lower = word.to_lowercase();
    if ABBREVIATIONS.contains(&lower.as_str()) {
        return true;
    }
    let line_start = before[..word_start].rfind('\n').map_or(0, |i| i + 1);
    // A `1.` that opens a line is a list marker.
    !word.is_empty()
        && word.chars().all(|c| c.is_ascii_digit())
        && before[line_start..word_start].trim().is_empty()
}

fn push_trimmed(out: &mut Vec<Range<usize>>, text: &str, range: Range<usize>) {
    let slice = &text[range.clone()];
    let lead = slice.len() - slice.trim_start().len();
    let trimmed = slice.trim();
    if trimmed.chars().any(|c| c.is_alphanumeric() || c == MASK) {
        let start = range.start + lead;
        out.push(start..start + trimmed.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(text: &str) -> Vec<&str> {
        sentences(text).into_iter().map(|r| &text[r]).collect()
    }

    #[test]
    fn corpus() {
        let cases: &[(&str, &[&str])] = &[
            ("One. Two! Three?", &["One.", "Two!", "Three?"]),
            (
                "lowercase starts count. like this one.",
                &["lowercase starts count.", "like this one."],
            ),
            (
                "Pins v1.2.3 of the client, e.g. for retries.",
                &["Pins v1.2.3 of the client, e.g. for retries."],
            ),
            (
                "Use i.e. this, etc. and so on.",
                &["Use i.e. this, etc. and so on."],
            ),
            (
                "Costs 3.50 now. Then more.",
                &["Costs 3.50 now.", "Then more."],
            ),
            (
                "Edit main.rs. Then build.",
                &["Edit main.rs.", "Then build."],
            ),
            (
                "No terminal punctuation here",
                &["No terminal punctuation here"],
            ),
            ("TODO\nfix this", &["TODO\nfix this"]),
            ("Wait... what?! Really.", &["Wait... what?!", "Really."]),
            (
                "run <file> [args ...]  (flags go last)",
                &["run <file> [args ...]  (flags go last)"],
            ),
            (
                "Shows \"Retry?\" after 5s, then gives up.",
                &["Shows \"Retry?\" after 5s, then gives up."],
            ),
            (
                "It said \u{201C}Stop!\u{201D} and froze. Next.",
                &["It said \u{201C}Stop!\u{201D} and froze.", "Next."],
            ),
            (
                "Then\u{2026} later. Done.",
                &["Then\u{2026} later.", "Done."],
            ),
            (
                "He said \"stop.\" Then left.",
                &["He said \"stop.\"", "Then left."],
            ),
            ("(See below.) Next.", &["(See below.)", "Next."]),
            (
                "Smith et al. found it. Done.",
                &["Smith et al. found it.", "Done."],
            ),
            ("Red vs. blue.", &["Red vs. blue."]),
            (
                "Steps:\n1. first\n2. second.",
                &["Steps:\n1. first\n2. second."],
            ),
            ("It was 2. Then 3.", &["It was 2.", "Then 3."]),
            ("", &[]),
            ("   ", &[]),
            ("... !", &[]),
            ("Ends with a dot.", &["Ends with a dot."]),
            ("  padded.   next  ", &["padded.", "next"]),
            ("Unicode “quoted.” Next.", &["Unicode “quoted.”", "Next."]),
            ("(e.g. this) works.", &["(e.g. this) works."]),
        ];
        for (text, want) in cases {
            assert_eq!(split(text), *want, "{text:?}");
        }
    }

    #[test]
    fn masked_code_counts_as_content() {
        let text = format!("{MASK}. Run it.");
        assert_eq!(count(&text), 2);
        assert_eq!(count(&format!("Call {MASK}.thing() now.")), 1);
    }

    #[test]
    fn containing_finds_the_sentence() {
        let text = "First one. Second one here. Third.";
        let r = containing(text, text.find("Second").unwrap() + 3);
        assert_eq!(&text[r], "Second one here.");
        let r = containing(text, text.find(" Second").unwrap());
        assert_eq!(
            &text[r], "Second one here.",
            "whitespace belongs to the next sentence"
        );
        assert_eq!(containing(text, 999), 0..text.len());
    }
}
