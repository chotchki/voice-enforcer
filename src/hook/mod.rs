//! `voice-enforcer hook`: the PreToolUse contract for Write, Edit and MultiEdit.

mod guard;
mod rebuild;

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use similar::{DiffTag, TextDiff};

use crate::disputes::{self, Sample};
use crate::env::Env;
use crate::rules::Finding;
use crate::types::Severity;
use crate::{judges, pipeline, ruleset};
use guard::{Guard, Verdict};
use rebuild::{EditInput, MultiEditInput, Rebuilt, WriteInput};

#[derive(Debug, Deserialize)]
struct Payload {
    session_id: Option<String>,
    cwd: Option<PathBuf>,
    tool_name: String,
    #[serde(default)]
    tool_input: serde_json::Value,
}

#[derive(Debug, PartialEq, Eq)]
enum Response {
    Deny(String),
    Context(String),
}

/// Always exits 0 or 1, never 2: a broken enforcer must not block writes.
pub(crate) fn run(
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    env: &Env,
) -> u8 {
    if env.judge_child {
        return 0;
    }
    let mut input = String::new();
    let decided = stdin
        .read_to_string(&mut input)
        .map_err(|e| format!("can't read the hook payload: {e}"))
        .and_then(|_| decide(&input, env));
    let output = match decided {
        Ok(None) => return 0,
        Ok(Some(Response::Deny(reason))) => json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            }
        }),
        Ok(Some(Response::Context(context))) => json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "additionalContext": context,
            }
        }),
        Err(message) => {
            let _ = writeln!(stderr, "voice-enforcer: {message}");
            return 1;
        }
    };
    let _ = writeln!(stdout, "{output}");
    0
}

fn decide(input: &str, env: &Env) -> Result<Option<Response>, String> {
    let payload: Payload =
        serde_json::from_str(input).map_err(|e| format!("not a PreToolUse payload: {e}"))?;
    let cwd = payload.cwd.clone().unwrap_or_else(|| env.cwd.clone());

    let (path, rebuilt) = match payload.tool_name.as_str() {
        "Write" => {
            let input: WriteInput = tool_input(&payload)?;
            let path = cwd.join(&input.file_path);
            let rebuilt = rebuild::write(&path, input.content);
            (path, rebuilt)
        }
        "Edit" => {
            let input: EditInput = tool_input(&payload)?;
            let path = cwd.join(&input.file_path);
            let rebuilt = rebuild::edit(&path, std::slice::from_ref(&input.edit));
            (path, rebuilt)
        }
        "MultiEdit" => {
            let input: MultiEditInput = tool_input(&payload)?;
            let path = cwd.join(&input.file_path);
            let rebuilt = rebuild::edit(&path, &input.edits);
            (path, rebuilt)
        }
        _ => return Ok(None),
    };
    let Some(Rebuilt { before, after }) = rebuilt else {
        return Ok(None);
    };

    let ruleset = match ruleset::load(&ruleset::locate(None, env), &env.home) {
        Ok(Some(ruleset)) => ruleset,
        Ok(None) => return Ok(None),
        Err(e) => {
            return Ok(Some(Response::Context(format!(
                "voice-enforcer: the ruleset failed to load, so nothing was checked. {e}"
            ))));
        }
    };

    let changed = changed_lines(before.as_deref(), &after);
    let touched = |f: &Finding| {
        changed
            .as_ref()
            .is_none_or(|c| (f.line..=f.end_line).any(|l| c.contains(&l)))
    };
    let evaluated = pipeline::evaluate(&ruleset, &path, &after);
    let mut findings: Vec<Finding> = evaluated.findings.into_iter().filter(touched).collect();
    let mut notes = Vec::new();
    // Judges only see a draft that got past the deterministic rules; a block means a rewrite.
    if !findings.iter().any(|f| f.severity == Severity::Block) {
        let cache = judges::Cache::new(&env.cache_dir(), env.now);
        let outcome = judges::run(&ruleset, &evaluated.spans, changed.as_ref(), &cache);
        findings.extend(outcome.findings.into_iter().filter(touched));
        notes = outcome.notes;
    }

    let state = env.state_dir();
    guard::prune(&state, env.now);
    disputes::prune(&state, env.now);
    let session = payload.session_id.as_deref().unwrap_or("unknown");
    let guard = Guard::new(&state, session, &path, ruleset.max_consecutive_blocks);
    let shown = display_path(&path, &cwd);
    let ids: Vec<String> = findings
        .iter()
        .map(|f| finding_id(session, &path, f))
        .collect();
    let samples: Vec<Sample> = findings
        .iter()
        .zip(&ids)
        .map(|(f, id)| Sample::new(id, f, &path, &evaluated.spans, &ruleset))
        .collect();
    disputes::save(&state, &samples);
    let hint = format!(
        "Think a finding is wrong? Run `{} dispute <id> --reason \"why\"`. It records the finding for the ruleset owner; it doesn't change this result.",
        crate::setup::exe_for_shell(env)
    );
    let list = |header: &str| listing(header, &findings, &ids, &notes, &shown, &hint);
    let blocks = findings
        .iter()
        .filter(|f| f.severity == Severity::Block)
        .count();
    let warns = findings.len() - blocks;

    if blocks == 0 {
        guard.on_clean();
        if findings.is_empty() && notes.is_empty() {
            return Ok(None);
        }
        let header = format!("voice-enforcer warnings for {shown} (the write went through):");
        return Ok(Some(Response::Context(list(&header))));
    }
    Ok(Some(match guard.on_block() {
        Verdict::Deny => Response::Deny(list(&format!(
            "voice-enforcer blocked this write to {shown}: {blocks} blocking and {warns} warning finding(s). Fix the blocking ones and retry."
        ))),
        Verdict::Tripped => Response::Context(list(&format!(
            "voice-enforcer loop guard tripped: {shown} was blocked {} times in a row this session, so its blocking findings are warnings this time. Fix them anyway, or tell the user a rule looks wrong.",
            ruleset.max_consecutive_blocks
        ))),
    }))
}

fn tool_input<T: DeserializeOwned>(payload: &Payload) -> Result<T, String> {
    T::deserialize(&payload.tool_input)
        .map_err(|e| format!("unexpected {} tool_input: {e}", payload.tool_name))
}

/// 1-based lines of `after` that are new or changed; `None` means every line (a new file).
fn changed_lines(before: Option<&str>, after: &str) -> Option<HashSet<usize>> {
    let before = before?;
    let diff = TextDiff::from_lines(before, after);
    Some(
        diff.ops()
            .iter()
            .filter(|op| matches!(op.tag(), DiffTag::Insert | DiffTag::Replace))
            .flat_map(similar::DiffOp::new_range)
            .map(|i| i + 1)
            .collect(),
    )
}

fn display_path(path: &Path, cwd: &Path) -> String {
    path.strip_prefix(cwd).unwrap_or(path).display().to_string()
}

/// The dispute id for a finding: the line is left out on purpose, so text a retry moved keeps
/// its id.
pub(crate) fn finding_id(session: &str, path: &Path, finding: &Finding) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let mut hash = Sha256::new();
    for part in [
        session.as_bytes(),
        path.as_os_str().as_encoded_bytes(),
        finding.rule_id.as_bytes(),
        finding.matched.as_bytes(),
    ] {
        hash.update(part);
        hash.update([0]);
    }
    hash.finalize()[..4]
        .iter()
        .fold(String::with_capacity(8), |mut id, b| {
            let _ = write!(id, "{b:02x}");
            id
        })
}

fn listing(
    header: &str,
    findings: &[Finding],
    ids: &[String],
    notes: &[String],
    shown: &str,
    hint: &str,
) -> String {
    use std::fmt::Write as _;
    let mut sorted: Vec<(&Finding, &String)> = findings.iter().zip(ids).collect();
    sorted.sort_by_key(|(f, _)| (f.severity, f.line));
    let mut out = header.to_owned();
    for (f, id) in &sorted {
        let _ = write!(
            out,
            "\n- [{}] {} at {shown}:{} (id {id}): \"{}\" - {}",
            f.severity, f.rule_id, f.line, f.matched, f.message
        );
        if let Some(s) = &f.suggestion {
            let _ = write!(out, " (suggestion: {s})");
        }
    }
    for note in notes {
        let _ = write!(out, "\n- note: {note}");
    }
    if !sorted.is_empty() {
        let _ = write!(out, "\n{hint}");
    }
    out
}

#[cfg(test)]
mod tests;
