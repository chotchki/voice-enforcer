//! The LLM judge layer: Haiku calls for the tells a phrase list can't name.

mod cache;
#[cfg(test)]
pub(crate) mod fake;
mod input;
mod runner;
mod verify;

use std::path::Path;

pub(crate) use cache::Cache;

use crate::changes::Changes;
use crate::extract::ProseSpan;
use crate::rules::Finding;
use crate::ruleset::Ruleset;
use input::Prepared;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub findings: Vec<Finding>,
    /// One line per judge that couldn't give a verdict, for `additionalContext`.
    pub notes: Vec<String>,
}

/// Runs every judge with changed prose to read, in parallel, through the cache.
pub(crate) fn run(
    ruleset: &Ruleset,
    path: &Path,
    spans: &[ProseSpan],
    changes: &Changes,
    cache: &Cache,
) -> Outcome {
    let mut outcome = Outcome::default();
    let mut jobs = Vec::new();
    for judge in ruleset.judges.iter().filter(|j| !j.excludes.is_match(path)) {
        match input::prepare(judge, spans, changes) {
            Prepared::Skip => {}
            Prepared::TooLarge(chars) => outcome.notes.push(format!(
                "judge `{}` skipped: its input was {chars} characters, over the {} limit",
                judge.id,
                input::MAX_INPUT_CHARS
            )),
            Prepared::Ready(input) => jobs.push((judge, input)),
        }
    }
    if jobs.is_empty() {
        return outcome;
    }
    cache.prune();

    let results: Vec<_> = std::thread::scope(|s| {
        let handles: Vec<_> = jobs
            .into_iter()
            .map(|(judge, input)| {
                let settings = judge.settings(&ruleset.judge_settings);
                s.spawn(move || {
                    let key = Cache::key(judge, &settings, &input.text);
                    let verdict = cache.get(&key).map_or_else(
                        || {
                            let fresh = runner::call(&settings, judge, &input.text);
                            if let Ok(v) = &fresh {
                                cache.put(&key, v);
                            }
                            fresh
                        },
                        Ok,
                    );
                    (judge, input, verdict)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("judge threads don't panic"))
            .collect()
    });

    for (judge, input, verdict) in results {
        match verdict {
            Ok(verdict) => outcome
                .findings
                .extend(verify::findings(judge, &verdict, &input, spans, changes)),
            Err(failure) => outcome.notes.push(format!(
                "judge `{}` {failure}, so it wasn't checked",
                judge.id
            )),
        }
    }
    outcome.findings.sort_by_key(|f| f.line);
    outcome
}

#[cfg(test)]
mod tests;
