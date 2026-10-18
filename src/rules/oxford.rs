use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

use crate::sentence;

static SERIAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i),\s+(?:and|or)\b").expect("valid serial comma regex"));

/// A lone opener like "Honestly," isn't a list item, so "Honestly, I fixed it, and it works" passes.
const INTRO_WORDS: &[&str] = &[
    "honestly",
    "however",
    "also",
    "so",
    "yes",
    "no",
    "well",
    "first",
    "second",
    "then",
    "still",
    "now",
    "finally",
    "plus",
    "sure",
    "ok",
    "okay",
    "again",
    "instead",
    "meanwhile",
    "otherwise",
    "thus",
    "hence",
    "therefore",
    "besides",
    "anyway",
    "actually",
    "basically",
    "sadly",
    "luckily",
    "unfortunately",
    "fortunately",
    "today",
    "yesterday",
    "tomorrow",
    "here",
    "there",
];

/// A serial comma and the list it closes, from the first item through the conjunction.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Serial {
    pub comma: Range<usize>,
    pub list: Range<usize>,
}

/// Each serial comma: a comma before the final `and`/`or` of a list of at least two earlier
/// comma-separated items, every item after the first at most `max_item_words` long.
pub(super) fn serial_commas(text: &str, max_item_words: usize) -> Vec<Serial> {
    let mut out = Vec::new();
    for range in sentence::sentences(text) {
        let s = &text[range.clone()];
        for m in SERIAL.find_iter(s) {
            let before = &s[..m.start()];
            let clause = before
                .rsplit([';', ':', '(', ')', '\u{2014}', '\u{2013}'])
                .next()
                .unwrap_or(before);
            let items: Vec<&str> = clause.split(',').map(str::trim).collect();
            if is_list(&items, max_item_words) {
                let comma = range.start + m.start();
                let lead = clause.len() - clause.trim_start().len();
                let start = range.start + before.len() - clause.len() + lead;
                out.push(Serial {
                    comma: comma..comma + 1,
                    list: start..range.start + m.end(),
                });
            }
        }
    }
    out
}

fn is_list(items: &[&str], max_item_words: usize) -> bool {
    if items.len() < 2 || items.iter().any(|i| i.is_empty()) {
        return false;
    }
    if items[1..]
        .iter()
        .any(|i| i.split_whitespace().count() > max_item_words)
    {
        return false;
    }
    let lone_intro = items.len() == 2 && INTRO_WORDS.contains(&items[0].to_lowercase().as_str());
    !lone_intro
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus() {
        let cases: &[(&str, usize)] = &[
            ("red, green, and blue", 1),
            ("red, green and blue", 0),
            ("I fixed the bug, and the build passes now.", 0),
            ("Honestly, I fixed it, and it works.", 0),
            ("Pick tea, coffee, or water.", 1),
            ("We support red, green, and blue today.", 1),
            ("Lint, test, build, and ship.", 1),
            ("one; two, and three", 0),
            ("Ranked: alpha, beta, and gamma.", 1),
            ("red, a very long item with far too many words, and blue", 0),
            (", , and nothing", 0),
            ("red, green, and blue. Cats, dogs, or birds.", 2),
            ("The plan\u{2014}red, green, and blue\u{2014}held.", 1),
            ("one \u{2013} two, and three", 0),
            ("Caf\u{e9}s, bars, and clubs", 1),
        ];
        for (text, want) in cases {
            assert_eq!(serial_commas(text, 4).len(), *want, "{text:?}");
        }
    }

    #[test]
    fn ranges_cover_the_comma_and_the_list() {
        for (text, list) in [
            ("red, green, and blue", "red, green, and"),
            (
                "We support red, green, or blue.",
                "We support red, green, or",
            ),
            ("Ranked: alpha, beta, and gamma.", "alpha, beta, and"),
            ("One. Pick tea, coffee, or water.", "Pick tea, coffee, or"),
        ] {
            let got = serial_commas(text, 4);
            assert_eq!(got.len(), 1, "{text}");
            assert_eq!(&text[got[0].comma.clone()], ",");
            assert_eq!(&text[got[0].list.clone()], list);
        }
    }

    /// Known misfires: a comma-bracketed aside before a clause reads as a list. Pinned so a
    /// fix shows up as a diff here; ad's design.md has what was tried.
    #[test]
    fn aside_before_a_clause_still_fires() {
        for text in [
            "Saves the file, open or not, and closes the tab.",
            "The extra bytes stay in memory, past the end, and nothing reads them.",
        ] {
            assert_eq!(serial_commas(text, 4).len(), 1, "{text}");
        }
    }

    #[test]
    fn item_word_limit_is_configurable() {
        let text = "fast red cars, slow blue trucks, and bikes";
        assert_eq!(serial_commas(text, 2).len(), 0);
        assert_eq!(serial_commas(text, 3).len(), 1);
    }
}
