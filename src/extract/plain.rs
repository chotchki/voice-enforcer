use super::{ProseSpan, SpanBuilder};
use crate::types::Register;

/// One span per blank-line-separated paragraph.
pub(super) fn extract(src: &str) -> Vec<ProseSpan> {
    let mut out = Vec::new();
    let mut cur: Option<SpanBuilder> = None;
    for (i, line) in src.lines().enumerate() {
        let n = i + 1;
        if line.trim().is_empty() {
            out.extend(cur.take().and_then(SpanBuilder::finish));
            continue;
        }
        let b = cur.get_or_insert_with(|| SpanBuilder::new(Register::LongForm, None));
        if b.len() > 0 {
            b.push("\n", n);
        }
        b.push_prose(line, n);
    }
    out.extend(cur.and_then(SpanBuilder::finish));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraphs_split_on_blank_lines() {
        let spans = extract("first line\nsecond line\n\n\nthird para\n");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "first line\nsecond line");
        assert_eq!(spans[0].line_at(spans[0].text.find("second").unwrap()), 2);
        assert_eq!(spans[1].first_line(), 5);
    }

    #[test]
    fn raw_keeps_urls() {
        let spans = extract("see https://x.io/a\nnext\n");
        assert_eq!(spans[0].raw, "see https://x.io/a\nnext");
        assert_eq!(spans[0].raw_line_at(spans[0].raw.find("next").unwrap()), 2);
    }

    #[test]
    fn empty_input() {
        assert_eq!(extract(""), []);
        assert_eq!(extract("\n  \n"), []);
    }
}
