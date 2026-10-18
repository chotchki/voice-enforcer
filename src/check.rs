//! `check` and `validate`: the same pipeline as the hook, run by hand or in CI.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::env::Env;
use crate::rules::Finding;
use crate::ruleset::{self, Ruleset};
use crate::types::Severity;
use crate::{extract, judges, pipeline};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Format {
    Text,
    Json,
}

/// Exit 0 when nothing blocks, 1 when something does, 2 when the run itself failed.
pub(crate) fn check(
    paths: &[PathBuf],
    ruleset_flag: Option<&Path>,
    format: Format,
    with_judges: bool,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    env: &Env,
) -> u8 {
    let ruleset = match load(ruleset_flag, env) {
        Ok(rs) => rs,
        Err(message) => {
            let _ = writeln!(stderr, "voice-enforcer: {message}");
            return 2;
        }
    };

    let mut files = Vec::new();
    for arg in paths {
        let path = env.cwd.join(arg);
        if path.is_dir() {
            files.extend(walk(&path, &ruleset));
        } else if path.is_file() {
            files.push(path);
        } else {
            let _ = writeln!(stderr, "voice-enforcer: can't read {}", arg.display());
            return 2;
        }
    }

    let cache = judges::Cache::new(&env.cache_dir(), env.now);
    let mut results: Vec<(String, Finding)> = Vec::new();
    for file in files {
        if extract::kind_for(&file).is_none() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let shown = file
            .strip_prefix(&env.cwd)
            .unwrap_or(&file)
            .display()
            .to_string();
        let evaluated = pipeline::evaluate(&ruleset, &file, &text);
        let mut found = evaluated.findings;
        if with_judges && !found.iter().any(|f| f.severity == Severity::Block) {
            let outcome = judges::run(&ruleset, &evaluated.spans, None, &cache);
            found.extend(outcome.findings);
            for note in outcome.notes {
                let _ = writeln!(stderr, "voice-enforcer: {shown}: {note}");
            }
        }
        results.extend(found.into_iter().map(|f| (shown.clone(), f)));
    }

    match format {
        Format::Text => {
            for (path, f) in &results {
                let _ = writeln!(
                    stdout,
                    "{path}:{}: {} {}: {} (\"{}\")",
                    f.line, f.severity, f.rule_id, f.message, f.matched
                );
                if let Some(s) = &f.suggestion {
                    let _ = writeln!(stdout, "    suggestion: {s}");
                }
            }
        }
        Format::Json => {
            let list: Vec<_> = results
                .iter()
                .map(|(path, f)| {
                    json!({
                        "path": path,
                        "line": f.line,
                        "end_line": f.end_line,
                        "severity": f.severity.as_str(),
                        "rule_id": f.rule_id,
                        "matched": f.matched,
                        "message": f.message,
                        "suggestion": f.suggestion,
                    })
                })
                .collect();
            let _ = writeln!(stdout, "{}", serde_json::Value::Array(list));
        }
    }
    u8::from(results.iter().any(|(_, f)| f.severity == Severity::Block))
}

pub(crate) fn validate(
    ruleset_flag: Option<&Path>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    env: &Env,
) -> u8 {
    match load(ruleset_flag, env) {
        Ok(rs) => {
            let path = ruleset::locate(ruleset_flag, env);
            let _ = writeln!(
                stdout,
                "{}: valid, {} rule(s), {} judge(s)",
                path.display(),
                rs.rules.len(),
                rs.judges.len()
            );
            0
        }
        Err(message) => {
            let _ = writeln!(stderr, "voice-enforcer: {message}");
            2
        }
    }
}

fn load(flag: Option<&Path>, env: &Env) -> Result<Ruleset, String> {
    let path = ruleset::locate(flag, env);
    match ruleset::load(&path, &env.home) {
        Ok(Some(rs)) => Ok(rs),
        Ok(None) => Err(format!("no ruleset at {}", path.display())),
        Err(e) => Err(e.to_string()),
    }
}

fn walk(dir: &Path, ruleset: &Ruleset) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = crate::walk::files(dir, ruleset).collect();
    files.sort();
    files
}

#[cfg(test)]
mod tests;
