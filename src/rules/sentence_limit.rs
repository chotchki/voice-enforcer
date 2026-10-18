use super::{Finding, display};
use crate::extract::ProseSpan;
use crate::ruleset::{Rule, SentenceLimits};
use crate::sentence;

const PREVIEW_CHARS: usize = 60;

/// One finding on the block's first line when it has more sentences than its level allows.
pub(super) fn check(rule: &Rule, limits: &SentenceLimits, span: &ProseSpan) -> Option<Finding> {
    let level = span.level?;
    let limit = limits.for_level(level)?;
    let count = sentence::count(&span.text);
    if count <= limit {
        return None;
    }
    let preview = display(&span.text);
    let matched = match preview.char_indices().nth(PREVIEW_CHARS) {
        Some((cut, _)) => format!("{}…", &preview[..cut]),
        None => preview,
    };
    Some(Finding {
        rule_id: rule.id.clone(),
        severity: rule.severity,
        line: span.first_line(),
        end_line: span.last_line(),
        matched,
        message: format!(
            "{} ({count} sentences, the {} limit is {limit})",
            rule.message,
            level.as_str()
        ),
        suggestion: rule.suggestion.clone(),
    })
}
