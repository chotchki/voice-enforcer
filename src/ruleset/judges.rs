//! `[judges]` and `[[judge]]`: configuration for the LLM judge layer.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use toml::Spanned;

use super::{Cx, LoadError};
use crate::types::{Register, Severity};

const EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JudgeSettings {
    pub model: String,
    pub effort: String,
    pub command: String,
    pub timeout: Duration,
}

impl Default for JudgeSettings {
    fn default() -> Self {
        Self {
            model: "haiku".into(),
            effort: "medium".into(),
            command: "claude".into(),
            timeout: Duration::from_secs(45),
        }
    }
}

/// What a judge reads: only the changed prose, or the whole file with the changes marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Diff,
    Whole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judge {
    pub id: String,
    pub severity: Severity,
    pub scope: Scope,
    pub registers: Vec<Register>,
    pub rubric: String,
}

impl Judge {
    pub fn applies_to(&self, register: Register) -> bool {
        self.registers.contains(&register)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawJudges {
    model: Option<Spanned<String>>,
    effort: Option<Spanned<String>>,
    command: Option<Spanned<String>>,
    timeout_secs: Option<Spanned<i64>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawJudge {
    id: Option<Spanned<String>>,
    severity: Option<Spanned<String>>,
    scope: Option<Spanned<String>>,
    registers: Option<Spanned<Vec<Spanned<String>>>>,
    rubric: Option<Spanned<String>>,
    rubric_file: Option<Spanned<String>>,
}

impl Cx<'_> {
    pub(super) fn judge_settings(
        &self,
        raw: Option<RawJudges>,
    ) -> Result<JudgeSettings, LoadError> {
        let defaults = JudgeSettings::default();
        let Some(raw) = raw else {
            return Ok(defaults);
        };
        let text = |value: Option<Spanned<String>>, field: &str, default: String| match value {
            None => Ok(default),
            Some(v) if v.get_ref().trim().is_empty() => Err(self.fail(
                Some(v.span()),
                format!("`judges.{field}` must not be empty"),
            )),
            Some(v) => Ok(v.into_inner()),
        };
        let effort = match raw.effort {
            Some(e) if !EFFORTS.contains(&e.get_ref().as_str()) => {
                return Err(self.fail(
                    Some(e.span()),
                    format!(
                        "unknown `judges.effort` `{}`; expected one of {}",
                        e.get_ref(),
                        EFFORTS.join(", ")
                    ),
                ));
            }
            other => text(other, "effort", defaults.effort)?,
        };
        let timeout = match raw.timeout_secs {
            None => defaults.timeout,
            Some(v) => Duration::from_secs(
                u64::try_from(*v.get_ref())
                    .ok()
                    .filter(|n| *n >= 1)
                    .ok_or_else(|| {
                        self.fail(Some(v.span()), "`judges.timeout_secs` must be at least 1")
                    })?,
            ),
        };
        Ok(JudgeSettings {
            model: text(raw.model, "model", defaults.model)?,
            effort,
            command: text(raw.command, "command", defaults.command)?,
            timeout,
        })
    }

    pub(super) fn judge(
        &self,
        raw: Spanned<RawJudge>,
        seen: &mut HashSet<String>,
    ) -> Result<Judge, LoadError> {
        let span = raw.span();
        let raw = raw.into_inner();
        let id = self.id(raw.id.as_ref(), &span, "judge", seen)?;
        let at = |field: &str| format!("judge `{id}`: {field}");

        let severity = self.severity(raw.severity.as_ref(), &span, &at)?;
        let scope = match raw.scope.as_ref().map(|s| (s.get_ref().as_str(), s)) {
            Some(("diff", _)) => Scope::Diff,
            Some(("whole", _)) => Scope::Whole,
            Some((other, s)) => {
                return Err(self.fail(
                    Some(s.span()),
                    at(&format!("unknown scope `{other}`; expected diff or whole")),
                ));
            }
            None => return Err(self.fail(Some(span), at("missing `scope`"))),
        };
        let registers = self.registers(
            raw.registers.as_ref(),
            vec![Register::LongForm, Register::Comment],
            &at,
        )?;

        let rubric = match (raw.rubric, raw.rubric_file) {
            (Some(_), Some(file)) => {
                return Err(self.fail(
                    Some(file.span()),
                    at("set `rubric` or `rubric_file`, not both"),
                ));
            }
            (None, None) => {
                return Err(self.fail(Some(span), at("needs `rubric` or `rubric_file`")));
            }
            (Some(inline), None) => inline,
            (None, Some(file)) => {
                let dir = self.path.parent().unwrap_or(Path::new(""));
                let text = std::fs::read_to_string(dir.join(file.get_ref())).map_err(|e| {
                    self.fail(
                        Some(file.span()),
                        at(&format!(
                            "can't read `rubric_file` `{}`: {e}",
                            file.get_ref()
                        )),
                    )
                })?;
                Spanned::new(file.span(), text)
            }
        };
        if rubric.get_ref().trim().is_empty() {
            return Err(self.fail(Some(rubric.span()), at("the rubric is empty")));
        }

        Ok(Judge {
            id,
            severity,
            scope,
            registers,
            rubric: rubric.get_ref().trim().to_owned(),
        })
    }
}

#[cfg(test)]
mod tests;
