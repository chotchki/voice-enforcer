//! Runs a ruleset's deterministic rules over prose spans.

mod file_links;
mod oxford;
mod sentence_limit;

use std::ops::Range;
use std::path::Path;

use crate::extract::{FileKind, MASK, ProseSpan, kind_for};
use crate::ruleset::{Rule, RuleKind, Ruleset};
use crate::sentence;
use crate::types::Severity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub rule_id: String,
    pub severity: Severity,
    /// 1-based line where the match starts.
    pub line: usize,
    /// 1-based line where the match ends; a finding counts as touched if any line in between changed.
    pub end_line: usize,
    pub matched: String,
    pub message: String,
    pub suggestion: Option<String>,
    /// Index of the prose span it came from, in the spans it was evaluated against.
    pub span: usize,
}

/// Every finding from every rule, ordered by line and then by rule order. `path` is the absolute
/// path of the file the spans came from, which file links resolve against.
pub fn evaluate(ruleset: &Ruleset, spans: &[ProseSpan], path: &Path) -> Vec<Finding> {
    let resolver = file_links::Resolver::new(path, ruleset, file_links::SEARCH_CAP);
    let is_markdown = kind_for(path) == Some(FileKind::Markdown);
    let mut out = Vec::new();
    for rule in ruleset.rules.iter().filter(|r| !r.excludes.is_match(path)) {
        for (index, span) in spans
            .iter()
            .enumerate()
            .filter(|(_, s)| rule.applies_to(s.register))
        {
            let from = out.len();
            match &rule.kind {
                RuleKind::Phrase {
                    phrases,
                    allow_when,
                } => {
                    for phrase in phrases {
                        for m in phrase.find_iter(&span.text) {
                            let sentence = &span.text[sentence::containing(&span.text, m.start())];
                            if !allow_when.iter().any(|a| a.is_match(sentence)) {
                                out.push(finding(rule, span, m.range()));
                            }
                        }
                    }
                }
                RuleKind::Regex { pattern } => out.extend(
                    pattern
                        .find_iter(&span.text)
                        .filter(|m| !m.is_empty())
                        .map(|m| finding(rule, span, m.range())),
                ),
                RuleKind::OxfordComma { max_item_words } => out.extend(
                    oxford::serial_commas(&span.text, *max_item_words)
                        .into_iter()
                        .map(|s| {
                            let mut f = finding(rule, span, s.comma);
                            f.matched = shorten(&display(&span.text[s.list]));
                            f
                        }),
                ),
                RuleKind::SentenceLimit { limits } => {
                    out.extend(sentence_limit::check(rule, limits, span));
                }
                RuleKind::FileLinks if is_markdown => {
                    out.extend(file_links::check(rule, span, &resolver));
                }
                RuleKind::FileLinks => {}
            }
            for f in &mut out[from..] {
                f.span = index;
            }
        }
    }
    out.sort_by_key(|f| f.line);
    out
}

fn finding(rule: &Rule, span: &ProseSpan, range: Range<usize>) -> Finding {
    Finding {
        rule_id: rule.id.clone(),
        severity: rule.severity,
        line: span.line_at(range.start),
        end_line: span.line_at(range.end.saturating_sub(1).max(range.start)),
        matched: display(&span.text[range]),
        message: rule.message.clone(),
        suggestion: rule.suggestion.clone(),
        span: 0,
    }
}

const MAX_MATCHED_CHARS: usize = 100;

fn shorten(text: &str) -> String {
    match text.char_indices().nth(MAX_MATCHED_CHARS) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_owned(),
    }
}

/// Masked text shown to a reader: each mask becomes a visible placeholder, whitespace collapses.
fn display(text: &str) -> String {
    text.replace(MASK, "`…`")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests;
