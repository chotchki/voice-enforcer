//! Disputes: the hook keeps a short-lived sample of each finding it lists, and `dispute` turns
//! one into a durable record in the ruleset directory. Record-only: nothing here is read back
//! when rules or judges run.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::env::Env;
use crate::extract::ProseSpan;
use crate::rules::Finding;
use crate::ruleset::{self, Ruleset};

const KEEP_FOR: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MAX_CONTEXT_CHARS: usize = 4_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    Rule,
    Judge,
}

/// Everything a ruleset owner needs to judge a finding later, without the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Sample {
    pub id: String,
    pub rule: String,
    pub kind: Kind,
    pub severity: String,
    pub file: String,
    pub line: usize,
    pub matched: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rubric_sha256: Option<String>,
    pub context: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_context: Option<String>,
}

impl Sample {
    pub(crate) fn new(
        id: &str,
        finding: &Finding,
        path: &Path,
        spans: &[ProseSpan],
        ruleset: &Ruleset,
    ) -> Self {
        let span = spans
            .iter()
            .find(|s| (s.first_line()..=s.last_line()).contains(&finding.line));
        let judge = ruleset.judges.iter().find(|j| j.id == finding.rule_id);
        let settings = &ruleset.judge_settings;
        Self {
            id: id.to_owned(),
            rule: finding.rule_id.clone(),
            kind: if judge.is_some() {
                Kind::Judge
            } else {
                Kind::Rule
            },
            severity: finding.severity.as_str().to_owned(),
            file: path.display().to_string(),
            line: finding.line,
            matched: finding.matched.clone(),
            message: finding.message.clone(),
            suggestion: finding.suggestion.clone(),
            model: judge.map(|_| settings.model.clone()),
            effort: judge.map(|_| settings.effort.clone()),
            rubric_sha256: judge.map(|j| short_sha256(&j.rubric)),
            context: span.map_or_else(String::new, |s| cap(&s.raw)),
            code_context: span.and_then(|s| s.code_context.as_deref()).map(cap),
        }
    }
}

fn samples_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("findings")
}

/// Best effort: a sample that can't be saved only means that finding can't be disputed.
pub(crate) fn save(state_dir: &Path, samples: &[Sample]) {
    let dir = samples_dir(state_dir);
    if samples.is_empty() || fs::create_dir_all(&dir).is_err() {
        return;
    }
    for sample in samples {
        let json = serde_json::to_string(sample).expect("a sample always serializes");
        let tmp = dir.join(format!(".{}.tmp-{}", sample.id, std::process::id()));
        if fs::write(&tmp, json).is_ok()
            && fs::rename(&tmp, dir.join(format!("{}.json", sample.id))).is_err()
        {
            let _ = fs::remove_file(&tmp);
        }
    }
}

pub(crate) fn load(state_dir: &Path, id: &str) -> Option<Sample> {
    if id.len() != 8 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let text = fs::read_to_string(samples_dir(state_dir).join(format!("{id}.json"))).ok()?;
    serde_json::from_str(&text).ok()
}

/// Deletes samples untouched for 7 days.
pub(crate) fn prune(state_dir: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(samples_dir(state_dir)) else {
        return;
    };
    for entry in entries.flatten() {
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| now.duration_since(t).unwrap_or_default() > KEEP_FOR);
        if stale {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// The durable record: the sample, the reason and when, in the ruleset's own format.
#[derive(Serialize)]
struct DisputeFile<'a> {
    version: u32,
    id: &'a str,
    rule: &'a str,
    kind: Kind,
    severity: &'a str,
    file: &'a str,
    line: usize,
    matched: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggestion: Option<&'a str>,
    reason: &'a str,
    disputed_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    effort: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rubric_sha256: Option<&'a str>,
    context: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    code_context: Option<&'a str>,
}

/// `voice-enforcer dispute`: exit 0 when recorded (or already recorded), 2 on bad input.
pub(crate) fn dispute(
    id: &str,
    reason: &str,
    ruleset_flag: Option<&Path>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    env: &Env,
) -> u8 {
    match record(id, reason, ruleset_flag, env) {
        Ok(message) => {
            let _ = writeln!(stdout, "{message}");
            0
        }
        Err(message) => {
            let _ = writeln!(stderr, "voice-enforcer: {message}");
            2
        }
    }
}

fn record(
    id: &str,
    reason: &str,
    ruleset_flag: Option<&Path>,
    env: &Env,
) -> Result<String, String> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err("a dispute needs a reason: --reason \"why the finding is wrong\"".into());
    }
    let sample = load(&env.state_dir(), id).ok_or_else(|| {
        format!("unknown or expired finding id `{id}` (samples are kept for 7 days)")
    })?;
    let ruleset_file = ruleset::locate(ruleset_flag, env);
    let dir = ruleset_file
        .parent()
        .expect("the ruleset file always sits in a directory")
        .join("disputes")
        .join(safe_dir_name(&sample.rule));

    let suffix = format!("-{id}.toml");
    if let Ok(entries) = fs::read_dir(&dir)
        && let Some(existing) = entries
            .flatten()
            .find(|e| e.file_name().to_string_lossy().ends_with(&suffix))
    {
        return Ok(format!("already disputed: {}", existing.path().display()));
    }

    let record = DisputeFile {
        version: 1,
        id: &sample.id,
        rule: &sample.rule,
        kind: sample.kind,
        severity: &sample.severity,
        file: &sample.file,
        line: sample.line,
        matched: &sample.matched,
        message: &sample.message,
        suggestion: sample.suggestion.as_deref(),
        reason,
        disputed_at: crate::clock::rfc3339(env.now),
        model: sample.model.as_deref(),
        effort: sample.effort.as_deref(),
        rubric_sha256: sample.rubric_sha256.as_deref(),
        context: &sample.context,
        code_context: sample.code_context.as_deref(),
    };
    let text = toml::to_string_pretty(&record).expect("a dispute always serializes");
    let path = dir.join(format!("{}{suffix}", crate::clock::utc_stamp(env.now)));
    let shown = path.display().to_string();
    // create_new: never overwrite a record, even if two disputes race.
    fs::create_dir_all(&dir)
        .and_then(|()| {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?
                .write_all(text.as_bytes())
        })
        .map_err(|e| format!("can't write {shown}: {e}"))?;
    Ok(format!("recorded the dispute in {shown}"))
}

/// Rule ids come from the ruleset, so anything that could climb out of `disputes/` is replaced.
fn safe_dir_name(id: &str) -> String {
    let name: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.chars().all(|c| c == '.') {
        format!("_{name}")
    } else {
        name
    }
}

fn cap(text: &str) -> String {
    match text.char_indices().nth(MAX_CONTEXT_CHARS) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_owned(),
    }
}

fn short_sha256(text: &str) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    Sha256::digest(text.as_bytes())[..8]
        .iter()
        .fold(String::with_capacity(16), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

#[cfg(test)]
mod tests;
