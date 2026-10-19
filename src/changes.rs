//! What a write changed: the lines holding a changed word, the words themselves and where they sit.

use std::collections::{HashMap, HashSet};
use std::ops::{Range, RangeInclusive};
use std::time::{Duration, Instant};

use similar::{Algorithm, DiffTag, TextDiff};

/// Past this the word diff approximates, which only ever marks more text as changed.
const WORD_DIFF_TIMEOUT: Duration = Duration::from_millis(100);

/// Source words a quote's alignment may skip between two of its own: a link target, inline HTML
/// or an entity puts words in the file that the judged text doesn't hold.
const MAX_SKIP: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Changes {
    /// `None` means every line changed (a new file, or `check`).
    lines: Option<HashMap<usize, LineChange>>,
    /// Byte ranges in `after` this write put there: each added or replaced token, and each
    /// deletion as the gap it left between the tokens around it.
    marks: Vec<Range<usize>>,
    after: String,
    starts: Vec<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LineChange {
    words: HashSet<String>,
    /// A deletion or a punctuation-only change: no word to match a quote against.
    opaque: bool,
}

impl Changes {
    pub(crate) fn all() -> Self {
        Self {
            lines: None,
            marks: Vec::new(),
            after: String::new(),
            starts: Vec::new(),
        }
    }

    /// Exactly these lines changed, in a way any quote on them overlaps.
    #[cfg(test)]
    pub(crate) fn on_lines(lines: &[usize]) -> Self {
        let opaque = LineChange {
            opaque: true,
            ..LineChange::default()
        };
        Self {
            lines: Some(lines.iter().map(|&l| (l, opaque.clone())).collect()),
            ..Self::all()
        }
    }

    /// `before` is `None` for a file that doesn't exist yet.
    pub(crate) fn between(before: Option<&str>, after: &str) -> Self {
        Self::with_timeout(before, after, WORD_DIFF_TIMEOUT)
    }

    fn with_timeout(before: Option<&str>, after: &str, timeout: Duration) -> Self {
        let Some(before) = before else {
            return Self::all();
        };
        let mut lines: HashMap<usize, LineChange> = HashMap::new();
        let mut marks = Vec::new();
        let old_starts = line_starts(before);
        let new_starts = line_starts(after);
        // A line diff finds the hunks; a word diff inside each one drops reflow and re-indentation.
        for (old, new) in hunks(&TextDiff::from_lines(before, after)) {
            if new.is_empty() {
                continue;
            }
            let old_text = &before[byte_range(&old_starts, before.len(), &old)];
            let new_bytes = byte_range(&new_starts, after.len(), &new);
            let base = new_bytes.start;
            let new_text = &after[new_bytes];
            // Whitespace stays out of the diff: kept in, a space can out-match a word for the LCS.
            let old_words = tokens(old_text);
            let new_words = tokens(new_text);
            let ops = similar::capture_diff_slices_deadline(
                Algorithm::Myers,
                &old_words.iter().map(|t| t.1).collect::<Vec<_>>(),
                &new_words.iter().map(|t| t.1).collect::<Vec<_>>(),
                Some(Instant::now() + timeout),
            );
            let line_of = |offset: usize| {
                let line = new.start + 1 + new_text[..offset].matches('\n').count();
                line.min(new.end)
            };
            for op in ops {
                match op.tag() {
                    DiffTag::Equal => {}
                    DiffTag::Insert | DiffTag::Replace => {
                        for &(at, token) in &new_words[op.new_range()] {
                            marks.push(base + at..base + at + token.len());
                            let change = lines.entry(line_of(at)).or_default();
                            let known = change.words.len();
                            change.words.extend(word_keys(token));
                            if change.words.len() == known && !has_words(token) {
                                change.opaque = true;
                            }
                        }
                    }
                    DiffTag::Delete => {
                        let i = op.new_range().start;
                        let at = new_words.get(i).map_or(new_text.len(), |t| t.0);
                        let gap_start = i
                            .checked_sub(1)
                            .map_or(0, |p| new_words[p].0 + new_words[p].1.len());
                        marks.push(base + gap_start..base + at);
                        lines.entry(line_of(at)).or_default().opaque = true;
                    }
                }
            }
        }
        Self {
            lines: Some(lines),
            marks,
            after: after.to_owned(),
            starts: new_starts,
        }
    }

    /// Whether any line in `lines` (1-based) changed; always true when every line did.
    pub(crate) fn touches(&self, lines: RangeInclusive<usize>) -> bool {
        self.lines
            .as_ref()
            .is_none_or(|map| lines.into_iter().any(|l| map.contains_key(&l)))
    }

    /// Whether `quote`, found on `lines`, holds text this write put there: a changed token or a
    /// deletion inside the quote, wherever it can be placed on those lines. A quote that can't be
    /// placed falls back to sharing a changed word with its lines, or any line with a deletion
    /// or a word-less change, which over-reports.
    pub(crate) fn quote_overlaps(&self, lines: RangeInclusive<usize>, quote: &str) -> bool {
        let Some(map) = &self.lines else {
            return true;
        };
        let windows = self.place(&lines, quote);
        if windows.is_empty() {
            let quoted: HashSet<String> = word_keys(quote).collect();
            return lines
                .into_iter()
                .filter_map(|l| map.get(&l))
                .any(|change| change.opaque || !change.words.is_disjoint(&quoted));
        }
        windows.iter().any(|w| {
            self.marks
                .iter()
                .any(|m| m.start <= w.end && w.start <= m.end)
        })
    }

    /// Every byte window on `lines` of `after` whose words hold `quote`'s words in order, from
    /// the start of the first one's token to the end of the last one's.
    fn place(&self, lines: &RangeInclusive<usize>, quote: &str) -> Vec<Range<usize>> {
        let wanted: Vec<String> = word_keys(quote).collect();
        let Some(first) = wanted.first() else {
            return Vec::new();
        };
        let span = byte_range(
            &self.starts,
            self.after.len(),
            &(lines.start().saturating_sub(1)..*lines.end()),
        );
        let base = span.start;
        let words: Vec<(String, Range<usize>)> = tokens(&self.after[span])
            .into_iter()
            .flat_map(|(at, token)| {
                word_keys(token).map(move |w| (w, base + at..base + at + token.len()))
            })
            .collect();
        let mut out = Vec::new();
        'starts: for start in (0..words.len()).filter(|&i| words[i].0 == *first) {
            let mut last = start;
            for want in &wanted[1..] {
                let next = words[last + 1..]
                    .iter()
                    .take(MAX_SKIP + 1)
                    .position(|(w, _)| w == want);
                match next {
                    Some(n) => last += n + 1,
                    None => continue 'starts,
                }
            }
            out.push(words[start].1.start..words[last].1.end);
        }
        out
    }
}

/// `(old lines, new lines)` of each non-equal line op, 0-based. `similar` already folds a delete
/// next to an insert into one replace, so a reflowed paragraph is one hunk.
fn hunks(diff: &TextDiff<'_, '_, str>) -> Vec<(Range<usize>, Range<usize>)> {
    diff.ops()
        .iter()
        .filter(|op| op.tag() != DiffTag::Equal)
        .map(|op| (op.old_range(), op.new_range()))
        .collect()
}

/// Each whitespace-separated token with its byte offset.
fn tokens(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                out.push((s, &text[s..i]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, &text[s..]));
    }
    out
}

fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(
            text.match_indices('\n')
                .map(|(i, _)| i + 1)
                .filter(|&i| i < text.len()),
        )
        .collect()
}

fn byte_range(starts: &[usize], len: usize, lines: &Range<usize>) -> Range<usize> {
    let at = |line: usize| starts.get(line).copied().unwrap_or(len);
    at(lines.start)..at(lines.end)
}

/// Lowercased runs of letters, digits and `_`: `**Bold**` and `[bold](x.md)` both hold `bold`.
fn word_keys(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
}

fn has_words(text: &str) -> bool {
    word_keys(text).next().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(c: &Changes) -> Vec<usize> {
        let mut out: Vec<usize> = c.lines.as_ref().unwrap().keys().copied().collect();
        out.sort_unstable();
        out
    }

    #[test]
    fn new_file_changes_everything() {
        let c = Changes::between(None, "a\nb\n");
        assert_eq!(c, Changes::all());
        assert!(c.touches(1..=1) && c.touches(99..=99));
        assert!(c.quote_overlaps(1..=1, "anything"));
    }

    #[test]
    fn changed_and_added_lines() {
        let c = Changes::between(Some("a\nb\nc\n"), "a\nB\nc\nd\n");
        assert_eq!(lines(&c), vec![2, 4]);
        assert!(c.touches(1..=2));
        assert!(!c.touches(1..=1));
        assert!(!c.touches(3..=3));
    }

    #[test]
    fn rewrapped_paragraph_changes_nothing() {
        let before = "One two three\nfour five six\nseven.\n";
        let after = "One two\nthree four five\nsix seven.\n";
        assert_eq!(
            lines(&Changes::between(Some(before), after)),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn rewrapping_onto_more_lines_changes_nothing() {
        let before = "x\nOne two three four\nfive six.\ny\n";
        let after = "x\nOne two\nthree four five\nsix.\ny\n";
        assert_eq!(
            lines(&Changes::between(Some(before), after)),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn reindented_comment_changes_nothing() {
        let before = "fn f() {\n        // keep the cache warm\n}\n";
        let after = "fn f() {\n    // keep the cache warm\n}\n";
        assert_eq!(
            lines(&Changes::between(Some(before), after)),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn deleting_a_word_touches_its_line() {
        let c = Changes::between(Some("x\nit really works\ny\n"), "x\nit works\ny\n");
        assert_eq!(lines(&c), vec![2]);
        assert!(c.quote_overlaps(2..=2, "it works"));
    }

    #[test]
    fn deleting_whole_lines_touches_nothing() {
        let c = Changes::between(Some("a\nb\nc\n"), "a\nc\n");
        assert_eq!(lines(&c), Vec::<usize>::new());
        let c = Changes::between(Some("a\nb\n"), "a\n");
        assert_eq!(lines(&c), Vec::<usize>::new());
    }

    #[test]
    fn deletion_at_the_end_lands_on_the_last_line() {
        let c = Changes::between(Some("keep\ntail words\n"), "keep\ntail\n");
        assert_eq!(lines(&c), vec![2]);
    }

    #[test]
    fn quotes_must_hold_a_changed_word() {
        let before = "First sentence stays. Second one is old.\n";
        let after = "First sentence stays. Second one is new.\n";
        let c = Changes::between(Some(before), after);
        assert!(!c.quote_overlaps(1..=1, "First sentence stays."));
        assert!(c.quote_overlaps(1..=1, "Second one is new."));
        assert!(!c.quote_overlaps(2..=2, "Second one is new."), "wrong line");
    }

    #[test]
    fn markup_around_a_word_still_matches() {
        let c = Changes::between(Some("say it\n"), "say **it** [loud](x.md)\n");
        assert!(c.quote_overlaps(1..=1, "say it loud"));
    }

    #[test]
    fn punctuation_on_a_word_changes_that_word() {
        let c = Changes::between(Some("red green and blue\n"), "red, green and blue\n");
        assert_eq!(lines(&c), vec![1]);
        assert!(c.quote_overlaps(1..=1, "red, green"));
        assert!(!c.quote_overlaps(1..=1, "and blue"));
    }

    #[test]
    fn a_bare_punctuation_change_counts_for_a_quote_spanning_it() {
        let c = Changes::between(Some("fast and cheap\n"), "fast - and cheap\n");
        assert_eq!(lines(&c), vec![1]);
        assert!(c.quote_overlaps(1..=1, "fast - and"));
        assert!(!c.quote_overlaps(1..=1, "and cheap"));
    }

    // One-line paragraphs: the change has to be inside the quote, not just on its line.
    const PARAGRAPH: &str = "The cache stays warm all day. Then a cold start costs a second.\n";

    #[test]
    fn a_common_word_elsewhere_on_the_line_doesnt_count() {
        let after = "The cache stays warm all day. Then the cold start costs a second.\n";
        let c = Changes::between(Some(PARAGRAPH), after);
        assert!(!c.quote_overlaps(1..=1, "The cache stays warm all day."));
        assert!(c.quote_overlaps(1..=1, "Then the cold start costs a second."));
    }

    #[test]
    fn a_deletion_elsewhere_on_the_line_doesnt_count() {
        let after = "The cache stays warm all day. Then a start costs a second.\n";
        let c = Changes::between(Some(PARAGRAPH), after);
        assert!(!c.quote_overlaps(1..=1, "The cache stays warm all day."));
    }

    #[test]
    fn a_deletion_inside_or_at_the_edge_of_the_quote_counts() {
        let after = "The cache stays warm. Then a cold start costs a second.\n";
        let c = Changes::between(Some(PARAGRAPH), after);
        assert!(c.quote_overlaps(1..=1, "The cache stays warm."));
        let c = Changes::between(Some("it really works\n"), "it works\n");
        assert!(
            c.quote_overlaps(1..=1, "works"),
            "the gap touches the quote"
        );
    }

    #[test]
    fn link_targets_are_skipped_when_placing_a_quote() {
        let before = "Read [the guide](docs/a.md) first. Then run a tool.\n";
        let after = "Read [the guide](docs/a.md) first. Then run the tool.\n";
        let c = Changes::between(Some(before), after);
        assert!(!c.quote_overlaps(1..=1, "Read the guide first."));
        let after = "Read [the new guide](docs/a.md) first. Then run a tool.\n";
        let c = Changes::between(Some(before), after);
        assert!(c.quote_overlaps(1..=1, "Read the new guide first."));
    }

    #[test]
    fn every_place_a_repeated_quote_fits_is_tried() {
        let before = "the cache warms. the cache cools.\n";
        let c = Changes::between(Some(before), "the cache warms. the Cache cools.\n");
        assert!(c.quote_overlaps(1..=1, "the cache"));
    }

    #[test]
    fn a_quote_can_run_across_lines() {
        let before = "First line here\nsecond line old.\n";
        let c = Changes::between(Some(before), "First line here\nsecond line new.\n");
        assert!(c.quote_overlaps(1..=2, "here second line new."));
        assert!(!c.quote_overlaps(1..=2, "First line here second"));
    }

    #[test]
    fn a_quote_that_wont_place_falls_back_to_shared_words() {
        let before = "caf&eacute; menu old\n";
        let c = Changes::between(Some(before), "caf&eacute; menu new\n");
        assert!(c.quote_overlaps(1..=1, "café menu new"));
        assert!(!c.quote_overlaps(1..=1, "café menu"));
        assert!(
            !c.quote_overlaps(1..=1, "..."),
            "no words to place or share"
        );
        let skipped = format!("start {} end\n", "x ".repeat(MAX_SKIP + 1));
        let c = Changes::between(Some(&skipped.replace("end", "fin")), &skipped);
        assert!(
            c.quote_overlaps(1..=1, "start end"),
            "too far apart to place"
        );
    }

    #[test]
    fn tokens_carry_offsets() {
        assert_eq!(tokens(" a  bc\nd"), vec![(1, "a"), (4, "bc"), (7, "d")]);
        assert_eq!(tokens(""), vec![]);
    }

    #[test]
    fn empty_file_before() {
        let c = Changes::between(Some(""), "new words\n\nmore\n");
        assert_eq!(lines(&c), vec![1, 3]);
    }

    #[test]
    fn a_blown_deadline_never_hides_a_change() {
        let before = "alpha beta gamma\n".repeat(200);
        let mut after = before.clone();
        after.push_str("delta\n");
        after.replace_range(0..5, "ALPHA");
        let c = Changes::with_timeout(Some(&before), &after, Duration::ZERO);
        assert!(c.touches(1..=1));
        assert!(c.touches(201..=201));
        assert!(c.quote_overlaps(1..=1, "ALPHA beta"));
    }

    #[test]
    fn line_helpers() {
        assert_eq!(line_starts("ab\ncd\n"), vec![0, 3]);
        assert_eq!(line_starts("ab\ncd"), vec![0, 3]);
        assert_eq!(line_starts(""), vec![0]);
        assert_eq!(byte_range(&[0, 3], 6, &(1..2)), 3..6);
        assert_eq!(
            word_keys("Don't `x_y`, OK?").collect::<Vec<_>>(),
            ["don", "t", "x_y", "ok"]
        );
    }
}
