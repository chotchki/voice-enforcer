use super::{Finding, display};
use crate::extract::ProseSpan;
use crate::ruleset::{Rule, SentenceLimits};
use crate::sentence;

const PREVIEW_CHARS: usize = 60;

/// One finding on the block's first line when it has more sentences than its level allows.
pub(super) fn check(rule: &Rule, limits: &SentenceLimits, span: &ProseSpan) -> Option<Finding> {
    let level = span.level?;
    let limit = limits.for_level(level)?;
    let count = sentence::count(&outside_lists(&span.text));
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
        span: 0,
    })
}

/// `text` without its list items: a list is structure, not a run of sentences. An item starts
/// at a `-`, `*`, `+`, `1.` or `1)` marker and runs to a blank line or the next item.
fn outside_lists(text: &str) -> String {
    let mut kept = Vec::new();
    let mut in_item = false;
    for line in text.lines() {
        if is_item(line) {
            in_item = true;
        } else if line.trim().is_empty() {
            in_item = false;
            kept.push(line);
        } else if !in_item {
            kept.push(line);
        }
    }
    kept.join("\n")
}

fn is_item(line: &str) -> bool {
    let line = line.trim_start();
    if let Some(rest) = line.strip_prefix(['-', '*', '+']) {
        return rest.starts_with(' ');
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    digits > 0
        && line[digits..]
            .strip_prefix(['.', ')'])
            .is_some_and(|rest| rest.starts_with(' '))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_items_are_left_out() {
        let text = "One. Two.\n- a. b.\n  wrapped. still item.\n* c.\n+ d.\n1. e.\n2) f.\n\nThree.";
        assert_eq!(outside_lists(text), "One. Two.\n\nThree.");
        assert_eq!(outside_lists("-not a list. Two."), "-not a list. Two.");
        assert_eq!(outside_lists("3.5 percent. Ok."), "3.5 percent. Ok.");
        assert_eq!(outside_lists("12x. Ok."), "12x. Ok.");
    }
}
