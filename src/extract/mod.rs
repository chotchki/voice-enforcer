//! Turns a file into masked prose spans: the only thing rules ever read.

mod code;
mod markdown;
mod plain;

use std::ops::Range;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::types::{Level, Register};

/// Stands in for inline code, URLs and inline HTML: no phrase matches it and it never ends a sentence.
pub const MASK: char = '\u{FFFC}';

pub use code::Lang;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Markdown,
    PlainText,
    Code(Lang),
}

pub fn kind_for(path: &Path) -> Option<FileKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "md" | "markdown" => Some(FileKind::Markdown),
        "txt" => Some(FileKind::PlainText),
        other => Lang::from_extension(other).map(FileKind::Code),
    }
}

/// Extracts every prose span from `src`, or nothing for unknown types and generated files.
/// `directives` are extra patterns for tool-directive comments that are never prose.
pub fn extract(path: &Path, src: &str, directives: &[Regex]) -> Vec<ProseSpan> {
    if is_generated(src) {
        return Vec::new();
    }
    match kind_for(path) {
        Some(FileKind::Markdown) => markdown::extract(src),
        Some(FileKind::PlainText) => plain::extract(src),
        Some(FileKind::Code(lang)) => code::extract(lang, src, directives),
        None => Vec::new(),
    }
}

fn is_generated(src: &str) -> bool {
    src.lines()
        .take(5)
        .any(|l| l.contains("@generated") || l.contains("DO NOT EDIT"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProseSpan {
    pub register: Register,
    pub level: Option<Level>,
    /// Masked text; offsets in annotations and `line_at` refer to it.
    pub text: String,
    /// `(text offset, 1-based source line)` at each point the source line changes.
    segments: Vec<(usize, usize)>,
    pub annotations: Vec<Annotation>,
}

impl ProseSpan {
    pub fn line_at(&self, offset: usize) -> usize {
        let i = self.segments.partition_point(|&(o, _)| o <= offset);
        self.segments[i.saturating_sub(1)].1
    }

    pub fn first_line(&self) -> usize {
        self.segments[0].1
    }

    pub fn last_line(&self) -> usize {
        self.segments[self.segments.len() - 1].1
    }
}

/// Unmasked facts the file-link rule needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Annotation {
    InlineCode {
        range: Range<usize>,
        content: String,
    },
    Link {
        range: Range<usize>,
        dest: String,
    },
}

pub(crate) struct SpanBuilder {
    span: ProseSpan,
}

static URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:https?|ftp)://[^\s<>]+").expect("valid URL regex"));

impl SpanBuilder {
    pub(crate) fn new(register: Register, level: Option<Level>) -> Self {
        Self {
            span: ProseSpan {
                register,
                level,
                text: String::new(),
                segments: Vec::new(),
                annotations: Vec::new(),
            },
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.span.text.len()
    }

    fn mark(&mut self, line: usize) {
        if self.span.segments.last().map(|s| s.1) != Some(line) {
            self.span.segments.push((self.span.text.len(), line));
        }
    }

    pub(crate) fn push(&mut self, s: &str, line: usize) {
        if !s.is_empty() {
            self.mark(line);
            self.span.text.push_str(s);
        }
    }

    pub(crate) fn push_mask(&mut self, line: usize) -> Range<usize> {
        self.mark(line);
        let start = self.span.text.len();
        self.span.text.push(MASK);
        start..self.span.text.len()
    }

    /// Pushes `s` with any bare URL masked out.
    pub(crate) fn push_prose(&mut self, s: &str, line: usize) {
        let mut last = 0;
        for m in URL.find_iter(s) {
            self.push(&s[last..m.start()], line);
            self.push_mask(line);
            last = m.end();
        }
        self.push(&s[last..], line);
    }

    pub(crate) fn annotate(&mut self, annotation: Annotation) {
        self.span.annotations.push(annotation);
    }

    pub(crate) fn finish(self) -> Option<ProseSpan> {
        (!self.span.text.trim().is_empty()).then_some(self.span)
    }
}

pub(crate) struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub(crate) fn new(src: &str) -> Self {
        let starts = std::iter::once(0)
            .chain(src.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self { starts }
    }

    /// 1-based line holding byte `offset`.
    pub(crate) fn line(&self, offset: usize) -> usize {
        self.starts.partition_point(|&s| s <= offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_extension() {
        assert_eq!(kind_for(Path::new("a/README.md")), Some(FileKind::Markdown));
        assert_eq!(kind_for(Path::new("x.MARKDOWN")), Some(FileKind::Markdown));
        assert_eq!(kind_for(Path::new("notes.txt")), Some(FileKind::PlainText));
        assert_eq!(kind_for(Path::new("data.csv")), None);
        assert_eq!(kind_for(Path::new("Makefile")), None);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_extension_is_unknown() {
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"x.m\xffd"));
        assert_eq!(kind_for(path), None);
    }

    #[test]
    fn unknown_type_yields_nothing() {
        assert_eq!(
            extract(Path::new("data.csv"), "a,b,c\nleverage,x,y\n", &[]),
            []
        );
    }

    #[test]
    fn generated_files_yield_nothing() {
        let src = "<!-- @generated by a tool -->\n\nSome prose here.\n";
        assert_eq!(extract(Path::new("x.md"), src, &[]), []);
        let src = "a\nb\nc\nd\n# DO NOT EDIT\nprose\n";
        assert_eq!(extract(Path::new("x.txt"), src, &[]), []);
        let src = "a\nb\nc\nd\ne\n@generated too late\n";
        assert_eq!(extract(Path::new("x.txt"), src, &[]).len(), 1);
    }

    #[test]
    fn dispatch_reaches_markdown() {
        let spans = extract(Path::new("README.md"), "# Title\n\nBody.\n", &[]);
        assert_eq!(spans.len(), 2);
    }

    #[test]
    fn line_index() {
        let idx = LineIndex::new("ab\ncd\n\nef");
        assert_eq!(idx.line(0), 1);
        assert_eq!(idx.line(2), 1);
        assert_eq!(idx.line(3), 2);
        assert_eq!(idx.line(6), 3);
        assert_eq!(idx.line(7), 4);
    }

    #[test]
    fn builder_tracks_lines_and_masks_urls() {
        let mut b = SpanBuilder::new(Register::LongForm, None);
        b.push_prose("see https://example.com/a.md now", 3);
        b.push(" ", 3);
        b.push("", 9);
        b.push("next", 4);
        let span = b.finish().unwrap();
        assert_eq!(span.text, format!("see {MASK} now next"));
        assert_eq!(span.first_line(), 3);
        assert_eq!(span.last_line(), 4);
        assert_eq!(span.line_at(0), 3);
        assert_eq!(span.line_at(span.text.len() - 1), 4);
    }

    #[test]
    fn whitespace_only_span_is_dropped() {
        let mut b = SpanBuilder::new(Register::LongForm, None);
        b.push("   ", 1);
        assert!(b.finish().is_none());
    }
}
