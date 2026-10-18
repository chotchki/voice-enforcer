use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use super::{Annotation, LineIndex, ProseSpan, SpanBuilder};
use crate::types::Register;

/// One span per block (paragraph, heading, list item text, table cell); code, HTML blocks and
/// front matter are skipped and inline code, inline HTML and URLs are masked.
pub(super) fn extract(src: &str) -> Vec<ProseSpan> {
    let idx = LineIndex::new(src);
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS;

    let mut out = Vec::new();
    let mut cur: Option<SpanBuilder> = None;
    let mut skip = 0usize;
    let mut links: Vec<(usize, String)> = Vec::new();

    for (event, range) in Parser::new_ext(src, options).into_offset_iter() {
        let line = idx.line(range.start);
        match event {
            Event::Start(Tag::CodeBlock(_) | Tag::HtmlBlock | Tag::MetadataBlock(_)) => {
                flush(&mut cur, &mut out);
                skip += 1;
            }
            Event::End(TagEnd::CodeBlock | TagEnd::HtmlBlock | TagEnd::MetadataBlock(_)) => {
                skip -= 1;
            }
            _ if skip > 0 => {}
            Event::Start(
                Tag::Paragraph
                | Tag::Heading { .. }
                | Tag::Item
                | Tag::List(_)
                | Tag::TableCell
                | Tag::BlockQuote(_)
                | Tag::FootnoteDefinition(_),
            )
            | Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::Item
                | TagEnd::List(_)
                | TagEnd::TableCell
                | TagEnd::BlockQuote(_)
                | TagEnd::FootnoteDefinition,
            )
            | Event::Rule => flush(&mut cur, &mut out),
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                let b = builder(&mut cur);
                links.push((b.len(), dest_url.into_string()));
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                if let Some((start, dest)) = links.pop() {
                    let b = builder(&mut cur);
                    let range = start..b.len();
                    b.annotate(Annotation::Link { range, dest });
                }
            }
            Event::Text(text) => builder(&mut cur).push_prose(&text, line),
            Event::Code(code) => {
                let b = builder(&mut cur);
                let range = b.push_mask(line);
                b.annotate(Annotation::InlineCode {
                    range,
                    content: code.into_string(),
                });
            }
            Event::InlineHtml(_)
            | Event::Html(_)
            | Event::FootnoteReference(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_) => {
                builder(&mut cur).push_mask(line);
            }
            Event::SoftBreak | Event::HardBreak => builder(&mut cur).push(" ", line),
            _ => {}
        }
    }
    flush(&mut cur, &mut out);
    out
}

fn builder(cur: &mut Option<SpanBuilder>) -> &mut SpanBuilder {
    cur.get_or_insert_with(|| SpanBuilder::new(Register::LongForm, None))
}

fn flush(cur: &mut Option<SpanBuilder>, out: &mut Vec<ProseSpan>) {
    if let Some(span) = cur.take().and_then(SpanBuilder::finish) {
        out.push(span);
    }
}

#[cfg(test)]
mod tests {
    use super::super::MASK;
    use super::*;

    fn texts(src: &str) -> Vec<String> {
        extract(src).into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn code_blocks_are_ignored() {
        let src = "Intro.\n\n```rust\nlet leverage = 1;\n```\n\n    indented leverage\n\nOutro.\n";
        assert_eq!(texts(src), vec!["Intro.", "Outro."]);
    }

    #[test]
    fn inline_code_is_masked_and_annotated() {
        let spans = extract("Run `leverage --now` first.\n");
        assert_eq!(spans[0].text, format!("Run {MASK} first."));
        assert_eq!(
            spans[0].annotations,
            vec![Annotation::InlineCode {
                range: 4..4 + MASK.len_utf8(),
                content: "leverage --now".into()
            }]
        );
    }

    #[test]
    fn headings_are_prose_with_lines() {
        let spans = extract("Para one.\n\n## The leverage heading\n");
        assert_eq!(spans[1].text, "The leverage heading");
        assert_eq!(spans[1].first_line(), 3);
        assert_eq!(spans[1].register, Register::LongForm);
        assert_eq!(spans[1].level, None);
    }

    #[test]
    fn links_keep_text_and_record_dest() {
        let spans = extract("See [the guide](docs/guide.md) and ![a chart](img/c.png).\n");
        let span = &spans[0];
        assert_eq!(span.text, "See the guide and a chart.");
        assert_eq!(
            span.annotations,
            vec![
                Annotation::Link {
                    range: 4..13,
                    dest: "docs/guide.md".into()
                },
                Annotation::Link {
                    range: 18..25,
                    dest: "img/c.png".into()
                },
            ]
        );
    }

    #[test]
    fn link_around_inline_code() {
        let spans = extract("Open [`src/main.rs`](src/main.rs).\n");
        let span = &spans[0];
        assert!(matches!(
            &span.annotations[..],
            [Annotation::InlineCode { .. }, Annotation::Link { dest, .. }] if dest == "src/main.rs"
        ));
    }

    #[test]
    fn front_matter_is_skipped() {
        assert_eq!(texts("---\ntitle: leverage\n---\n\nBody.\n"), vec!["Body."]);
        assert_eq!(texts("+++\ntitle = 'x'\n+++\n\nBody.\n"), vec!["Body."]);
    }

    #[test]
    fn html_is_masked_or_skipped() {
        assert_eq!(texts("<div>\nleverage\n</div>\n\nAfter.\n"), vec!["After."]);
        assert_eq!(texts("a <br> b\n"), vec![format!("a {MASK} b")]);
    }

    #[test]
    fn urls_are_masked() {
        assert_eq!(
            texts("Go to https://example.com/leverage now. <https://x.io>\n"),
            vec![format!("Go to {MASK} now. {MASK}")]
        );
    }

    #[test]
    fn lists_tables_and_quotes_split_into_spans() {
        let src = "\
- tight one
- tight two
  - nested

> quoted

| a | b |
|---|---|
| c | d |

---

1. loose

2. second[^n]

[^n]: footnote body
";
        assert_eq!(
            texts(src),
            vec![
                "tight one".to_string(),
                "tight two".into(),
                "nested".into(),
                "quoted".into(),
                "a".into(),
                "b".into(),
                "c".into(),
                "d".into(),
                "loose".into(),
                format!("second{MASK}"),
                "footnote body".into(),
            ]
        );
    }

    #[test]
    fn wrapped_paragraph_maps_lines() {
        let spans = extract("\nWe got there at the end\nof the day, finally.\n");
        let span = &spans[0];
        assert_eq!(span.text, "We got there at the end of the day, finally.");
        assert_eq!(span.first_line(), 2);
        assert_eq!(span.line_at(span.text.find("of the").unwrap()), 3);
        assert_eq!(span.line_at(span.text.find("at the end").unwrap()), 2);
        assert_eq!(span.last_line(), 3);
    }

    #[test]
    fn emphasis_is_transparent() {
        assert_eq!(
            texts("A **bold** and _soft_ ~~gone~~ word.\n"),
            vec!["A bold and soft gone word."]
        );
    }

    #[test]
    fn task_list_markers_are_ignored() {
        assert_eq!(texts("- [x] done item\n"), vec!["done item"]);
    }
}
