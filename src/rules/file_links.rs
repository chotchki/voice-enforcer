use std::cell::OnceCell;
use std::path::{Component, Path, PathBuf};

use super::{Finding, display};
use crate::extract::{Annotation, MASK, ProseSpan};
use crate::ruleset::{Rule, Ruleset};
use crate::types::Severity;

pub(super) const SEARCH_CAP: usize = 50_000;

/// Extensions that make a bare name in backticks a file mention even when it can't be found.
const FILE_EXTENSIONS: &[&str] = &[
    "md",
    "markdown",
    "txt",
    "rs",
    "toml",
    "json",
    "jsonc",
    "yaml",
    "yml",
    "lock",
    "py",
    "ts",
    "tsx",
    "js",
    "jsx",
    "mjs",
    "cjs",
    "swift",
    "sh",
    "bash",
    "zsh",
    "html",
    "css",
    "scss",
    "sql",
    "go",
    "c",
    "h",
    "cc",
    "cpp",
    "hpp",
    "java",
    "kt",
    "rb",
    "php",
    "cs",
    "xml",
    "csv",
    "tsv",
    "png",
    "jpg",
    "jpeg",
    "gif",
    "svg",
    "pdf",
    "ini",
    "cfg",
    "conf",
    "env",
    "scad",
    "stl",
    "3mf",
    "docx",
    "odt",
    "xlsx",
    "proto",
    "graphql",
    "dockerfile",
    "mk",
    "cmake",
    "gradle",
    "plist",
];

const TRIM: &[char] = &[
    '(', ')', '[', ']', '{', '}', '"', '\'', '<', '>', ',', ';', ':', '!', '?', '.', '*', '_',
];

pub(super) struct Resolver<'a> {
    md_dir: PathBuf,
    root: Option<PathBuf>,
    ruleset: &'a Ruleset,
    cap: usize,
    index: OnceCell<Index>,
}

struct Index {
    /// Repository-relative paths.
    files: Vec<PathBuf>,
    truncated: bool,
}

enum Resolution {
    Found(PathBuf),
    /// More than one file matches, so there's no one link to suggest.
    Ambiguous(usize),
    Directory,
    Missing {
        truncated: bool,
    },
}

impl<'a> Resolver<'a> {
    pub(super) fn new(md_path: &Path, ruleset: &'a Ruleset, cap: usize) -> Self {
        let md_dir = md_path.parent().unwrap_or(Path::new("/")).to_path_buf();
        let root = md_dir
            .ancestors()
            .find(|d| d.join(".git").exists())
            .map(Path::to_path_buf);
        Self {
            md_dir,
            root,
            ruleset,
            cap,
            index: OnceCell::new(),
        }
    }

    fn resolve(&self, mention: &str) -> Resolution {
        let relative = self.md_dir.join(mention);
        if relative.is_file() {
            return Resolution::Found(normalize(&relative));
        }
        let at_root = self.root.as_ref().map(|r| r.join(mention));
        if relative.is_dir() || at_root.as_ref().is_some_and(|p| p.is_dir()) {
            return Resolution::Directory;
        }
        let (Some(root), Some(at_root)) = (&self.root, at_root) else {
            return Resolution::Missing { truncated: false };
        };
        if at_root.is_file() {
            return Resolution::Found(normalize(&at_root));
        }
        let index = self.index.get_or_init(|| self.build_index(root));
        let wanted: Vec<Component> = Path::new(mention).components().collect();
        let hits: Vec<&PathBuf> = index
            .files
            .iter()
            .filter(|f| f.components().collect::<Vec<_>>().ends_with(&wanted))
            .collect();
        match hits.as_slice() {
            [hit] => Resolution::Found(root.join(hit)),
            [] => Resolution::Missing {
                truncated: index.truncated,
            },
            many => Resolution::Ambiguous(many.len()),
        }
    }

    fn build_index(&self, root: &Path) -> Index {
        let mut files = Vec::new();
        let mut truncated = false;
        for path in crate::walk::files(root, self.ruleset) {
            if files.len() == self.cap {
                truncated = true;
                break;
            }
            if let Ok(rel) = path.strip_prefix(root) {
                files.push(rel.to_path_buf());
            }
        }
        Index { files, truncated }
    }
}

pub(super) fn check(rule: &Rule, span: &ProseSpan, resolver: &Resolver) -> Vec<Finding> {
    let links: Vec<_> = span
        .annotations
        .iter()
        .filter_map(|a| match a {
            Annotation::Link { range, dest } => Some((range.clone(), dest.as_str())),
            Annotation::InlineCode { .. } => None,
        })
        .collect();
    let in_link = |offset: usize| links.iter().any(|(r, _)| r.contains(&offset));
    let mut out = Vec::new();

    for (range, dest) in &links {
        if let Some(target) = local_target(dest)
            && !resolver.md_dir.join(&target).exists()
        {
            out.push(make(
                rule,
                span,
                range.start,
                Severity::Warn,
                display(&span.text[range.clone()]),
                format!("link target `{dest}` not found"),
                None,
            ));
        }
    }

    for annotation in &span.annotations {
        if let Annotation::InlineCode { range, content } = annotation
            && !in_link(range.start)
            && let Some(kind) = classify(content.trim(), true)
        {
            report(
                rule,
                span,
                resolver,
                range.start,
                content.trim(),
                kind,
                true,
                &mut out,
            );
        }
    }

    let mut offset = 0;
    for token in span.text.split_inclusive(char::is_whitespace) {
        let start = offset;
        offset += token.len();
        let lead = token.len() - token.trim_start_matches(TRIM).len();
        let word = token.trim().trim_matches(TRIM);
        if word.contains(MASK) || in_link(start + lead) {
            continue;
        }
        if let Some(kind) = classify(word, false) {
            report(
                rule,
                span,
                resolver,
                start + lead,
                word,
                kind,
                false,
                &mut out,
            );
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mention {
    /// Holds a `/`: warn when it can't be found.
    Path,
    /// A bare name that should warn when it can't be found.
    KnownName,
    /// A bare name that only counts if it resolves.
    MaybeName,
}

fn classify(word: &str, in_code: bool) -> Option<Mention> {
    if word.is_empty()
        || word.contains("://")
        || word.starts_with(['/', '~', '-'])
        || !word
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | '@' | '+'))
    {
        return None;
    }
    let ext = Path::new(word).extension()?.to_str()?;
    if !ext.chars().any(char::is_alphabetic) {
        return None;
    }
    if word.contains('/') {
        Some(Mention::Path)
    } else if in_code && FILE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()) {
        Some(Mention::KnownName)
    } else {
        Some(Mention::MaybeName)
    }
}

#[allow(clippy::too_many_arguments)]
fn report(
    rule: &Rule,
    span: &ProseSpan,
    resolver: &Resolver,
    offset: usize,
    mention: &str,
    kind: Mention,
    in_code: bool,
    out: &mut Vec<Finding>,
) {
    match resolver.resolve(mention) {
        Resolution::Found(target) => {
            let rel = relative(&resolver.md_dir, &target);
            let text = if in_code {
                format!("`{mention}`")
            } else {
                mention.to_owned()
            };
            let rel = if rel.contains(' ') {
                format!("<{rel}>")
            } else {
                rel
            };
            out.push(make(
                rule,
                span,
                offset,
                rule.severity,
                mention.to_owned(),
                rule.message.clone(),
                Some(format!("link it: [{text}]({rel})")),
            ));
        }
        Resolution::Missing { truncated } if kind != Mention::MaybeName => {
            let note = if truncated {
                format!(" (the search stopped after {} files)", resolver.cap)
            } else {
                String::new()
            };
            out.push(make(
                rule,
                span,
                offset,
                Severity::Warn,
                mention.to_owned(),
                format!("named file `{mention}` not found{note}"),
                None,
            ));
        }
        Resolution::Ambiguous(n) if kind != Mention::MaybeName => out.push(make(
            rule,
            span,
            offset,
            Severity::Warn,
            mention.to_owned(),
            format!("named file `{mention}` matches {n} files, so it can't be linked"),
            Some("name it by its path".to_owned()),
        )),
        Resolution::Missing { .. } | Resolution::Ambiguous(_) | Resolution::Directory => {}
    }
}

fn make(
    rule: &Rule,
    span: &ProseSpan,
    offset: usize,
    severity: Severity,
    matched: String,
    message: String,
    suggestion: Option<String>,
) -> Finding {
    let line = span.line_at(offset);
    Finding {
        rule_id: rule.id.clone(),
        severity,
        line,
        end_line: line,
        matched,
        message,
        suggestion,
        span: 0,
    }
}

/// The local file a link points at, or `None` for URLs, anchors, absolute and home paths.
fn local_target(dest: &str) -> Option<PathBuf> {
    let path = dest.split(['#', '?']).next().unwrap_or_default();
    if path.is_empty()
        || dest.contains("://")
        || dest.starts_with("mailto:")
        || path.starts_with(['/', '~'])
    {
        return None;
    }
    Some(PathBuf::from(percent_decode(path)))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Lexically resolves `.` and `..` so suggestions don't carry them.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// `to` relative to `from_dir`, with `/` separators.
fn relative(from_dir: &Path, to: &Path) -> String {
    let from_norm = normalize(from_dir);
    let to_norm = normalize(to);
    let from: Vec<_> = from_norm.components().collect();
    let to: Vec<_> = to_norm.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_owned(); from.len() - common];
    parts.extend(
        to[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

#[cfg(test)]
mod tests;
