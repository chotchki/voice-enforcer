//! Finds, parses and validates `ruleset.toml`, turning it into compiled rules.
//!
//! Parsing is two-stage: serde reads every value as `Spanned` into a flat raw struct, then
//! conversion checks it and builds the typed rules. A serde tagged enum would buffer its content
//! and lose the spans that let every error name a line.

use std::collections::HashSet;
use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use regex::Regex;
use serde::Deserialize;
use toml::Spanned;

use crate::env::Env;
use crate::types::{Level, Register, Severity};

mod judges;
pub use judges::{Judge, JudgeSettings, Scope};

pub const FILE_NAME: &str = "ruleset.toml";
pub const SUPPORTED_VERSION: i64 = 1;
const DEFAULT_MAX_CONSECUTIVE_BLOCKS: u32 = 3;
const DEFAULT_MAX_ITEM_WORDS: usize = 4;

const BUILTIN_EXCLUDES: &[&str] = &[
    "**/Cargo.lock",
    "**/package-lock.json",
    "**/yarn.lock",
    "**/pnpm-lock.yaml",
    "**/Gemfile.lock",
    "**/poetry.lock",
    "**/*.min.js",
    "**/target/**",
    "**/node_modules/**",
    "**/vendor/**",
    "**/dist/**",
    "**/.git/**",
];

const KINDS: &str = "phrase, regex, oxford_comma, sentence_limit, file_links";

#[derive(Debug)]
pub struct Ruleset {
    excludes: GlobSet,
    pub comment_directives: Vec<Regex>,
    pub max_consecutive_blocks: u32,
    pub rules: Vec<Rule>,
    pub judge_settings: JudgeSettings,
    pub judges: Vec<Judge>,
}

impl Ruleset {
    pub fn is_excluded(&self, path: &Path) -> bool {
        self.excludes.is_match(path)
    }

    /// An owned copy of the excludes, for walkers that need a `'static` filter.
    pub fn exclude_set(&self) -> GlobSet {
        self.excludes.clone()
    }
}

#[derive(Debug)]
pub struct Rule {
    pub id: String,
    pub severity: Severity,
    pub registers: Vec<Register>,
    pub message: String,
    pub suggestion: Option<String>,
    pub kind: RuleKind,
}

impl Rule {
    pub fn applies_to(&self, register: Register) -> bool {
        self.registers.contains(&register)
    }
}

#[derive(Debug)]
pub enum RuleKind {
    Phrase {
        phrases: Vec<Regex>,
        allow_when: Vec<Regex>,
    },
    Regex {
        pattern: Regex,
    },
    OxfordComma {
        max_item_words: usize,
    },
    SentenceLimit {
        limits: SentenceLimits,
    },
    FileLinks,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SentenceLimits {
    pub line: Option<usize>,
    pub item: Option<usize>,
    pub module: Option<usize>,
}

impl SentenceLimits {
    pub fn for_level(&self, level: Level) -> Option<usize> {
        match level {
            Level::Line => self.line,
            Level::Item => self.item,
            Level::Module => self.module,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    pub path: PathBuf,
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{line}: {}", self.path.display(), self.message),
            None => write!(f, "{}: {}", self.path.display(), self.message),
        }
    }
}

/// Resolves the ruleset file: `--ruleset`, then `VOICE_ENFORCER_RULESET`, then the XDG config dir.
pub fn locate(flag: Option<&Path>, env: &Env) -> PathBuf {
    let dir = flag
        .map(Path::to_path_buf)
        .or_else(|| env.ruleset_dir.clone())
        .unwrap_or_else(|| env.config_dir());
    env.cwd.join(dir).join(FILE_NAME)
}

/// Returns `Ok(None)` when no ruleset exists at `path`.
pub fn load(path: &Path, home: &Path) -> Result<Option<Ruleset>, LoadError> {
    match std::fs::read_to_string(path) {
        Ok(src) => parse(&src, path, home).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(LoadError {
            path: path.to_path_buf(),
            line: None,
            message: format!("can't read the ruleset: {e}"),
        }),
    }
}

pub fn parse(src: &str, path: &Path, home: &Path) -> Result<Ruleset, LoadError> {
    let cx = Cx { src, path, home };
    let raw: RawRuleset =
        toml::from_str(src).map_err(|e| cx.fail(e.span(), e.message().trim_end()))?;
    cx.convert(raw)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRuleset {
    version: Option<Spanned<i64>>,
    #[serde(default)]
    exclude: Vec<Spanned<String>>,
    #[serde(default)]
    comment_directives: Vec<Spanned<String>>,
    loop_guard: Option<RawLoopGuard>,
    #[serde(default)]
    rule: Vec<Spanned<RawRule>>,
    judges: Option<judges::RawJudges>,
    #[serde(default)]
    judge: Vec<Spanned<judges::RawJudge>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLoopGuard {
    max_consecutive_blocks: Option<Spanned<i64>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    id: Option<Spanned<String>>,
    kind: Option<Spanned<String>>,
    severity: Option<Spanned<String>>,
    registers: Option<Spanned<Vec<Spanned<String>>>>,
    message: Option<Spanned<String>>,
    suggestion: Option<Spanned<String>>,
    phrases: Option<Spanned<Vec<Spanned<String>>>>,
    allow_when: Option<Spanned<Vec<Spanned<String>>>>,
    pattern: Option<Spanned<String>>,
    max_item_words: Option<Spanned<i64>>,
    max_sentences: Option<Spanned<RawLimits>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLimits {
    line: Option<Spanned<i64>>,
    item: Option<Spanned<i64>>,
    module: Option<Spanned<i64>>,
}

struct Cx<'a> {
    src: &'a str,
    path: &'a Path,
    home: &'a Path,
}

impl Cx<'_> {
    fn fail(&self, span: Option<Range<usize>>, message: impl Into<String>) -> LoadError {
        LoadError {
            path: self.path.to_path_buf(),
            line: span.map(|s| line_of(self.src, s.start)),
            message: message.into(),
        }
    }

    fn convert(&self, raw: RawRuleset) -> Result<Ruleset, LoadError> {
        match raw.version {
            None => {
                return Err(self.fail(
                    None,
                    format!("missing `version`; this binary supports version {SUPPORTED_VERSION}"),
                ));
            }
            Some(v) if *v.get_ref() != SUPPORTED_VERSION => {
                return Err(self.fail(
                    Some(v.span()),
                    format!(
                        "unsupported version {}; this binary supports version {SUPPORTED_VERSION}",
                        v.get_ref()
                    ),
                ));
            }
            Some(_) => {}
        }

        let mut globs = GlobSetBuilder::new();
        for pattern in BUILTIN_EXCLUDES {
            globs.add(glob(pattern).expect("built-in globs are valid"));
        }
        for pattern in &raw.exclude {
            let expanded = expand_home(pattern.get_ref(), self.home);
            let compiled = glob(&expanded).map_err(|e| {
                self.fail(
                    Some(pattern.span()),
                    format!("invalid glob `{}` in `exclude`: {e}", pattern.get_ref()),
                )
            })?;
            globs.add(compiled);
        }
        let excludes = globs.build().map_err(|e| self.fail(None, e.to_string()))?;

        let comment_directives = raw
            .comment_directives
            .iter()
            .map(|p| self.regex(p, "`comment_directives`"))
            .collect::<Result<_, _>>()?;

        let max_consecutive_blocks = match raw.loop_guard.and_then(|g| g.max_consecutive_blocks) {
            None => DEFAULT_MAX_CONSECUTIVE_BLOCKS,
            Some(v) => u32::try_from(*v.get_ref())
                .ok()
                .filter(|n| *n >= 1)
                .ok_or_else(|| {
                    self.fail(
                        Some(v.span()),
                        "`loop_guard.max_consecutive_blocks` must be at least 1",
                    )
                })?,
        };

        let mut seen = HashSet::new();
        let rules = raw
            .rule
            .into_iter()
            .map(|r| self.rule(r, &mut seen))
            .collect::<Result<_, _>>()?;
        let judge_settings = self.judge_settings(raw.judges)?;
        let judges = raw
            .judge
            .into_iter()
            .map(|j| self.judge(j, &mut seen))
            .collect::<Result<_, _>>()?;

        Ok(Ruleset {
            excludes,
            comment_directives,
            max_consecutive_blocks,
            rules,
            judge_settings,
            judges,
        })
    }

    /// A non-empty id not yet used by any rule or judge (they share one namespace on findings).
    fn id(
        &self,
        raw: Option<&Spanned<String>>,
        table_span: &Range<usize>,
        what: &str,
        seen: &mut HashSet<String>,
    ) -> Result<String, LoadError> {
        let id = raw
            .filter(|id| !id.get_ref().trim().is_empty())
            .ok_or_else(|| {
                self.fail(Some(table_span.clone()), format!("{what} is missing `id`"))
            })?;
        if !seen.insert(id.get_ref().clone()) {
            return Err(self.fail(
                Some(id.span()),
                format!(
                    "duplicate {what} id `{}` (rules and judges share one set of ids)",
                    id.get_ref()
                ),
            ));
        }
        Ok(id.get_ref().clone())
    }

    fn severity(
        &self,
        raw: Option<&Spanned<String>>,
        table_span: &Range<usize>,
        at: &dyn Fn(&str) -> String,
    ) -> Result<Severity, LoadError> {
        match raw.map(|s| (s.get_ref().as_str(), s)) {
            Some(("block", _)) => Ok(Severity::Block),
            Some(("warn", _)) => Ok(Severity::Warn),
            Some((other, s)) => Err(self.fail(
                Some(s.span()),
                at(&format!(
                    "unknown severity `{other}`; expected block or warn"
                )),
            )),
            None => Err(self.fail(Some(table_span.clone()), at("missing `severity`"))),
        }
    }

    fn registers(
        &self,
        raw: Option<&Spanned<Vec<Spanned<String>>>>,
        default: Vec<Register>,
        at: &dyn Fn(&str) -> String,
    ) -> Result<Vec<Register>, LoadError> {
        match raw {
            None => Ok(default),
            Some(list) if list.get_ref().is_empty() => {
                Err(self.fail(Some(list.span()), at("`registers` must not be empty")))
            }
            Some(list) => list
                .get_ref()
                .iter()
                .map(|r| match r.get_ref().as_str() {
                    "long_form" => Ok(Register::LongForm),
                    "comment" => Ok(Register::Comment),
                    other => Err(self.fail(
                        Some(r.span()),
                        at(&format!(
                            "unknown register `{other}`; expected long_form or comment"
                        )),
                    )),
                })
                .collect(),
        }
    }

    fn regex(&self, pattern: &Spanned<String>, place: &str) -> Result<Regex, LoadError> {
        Regex::new(pattern.get_ref()).map_err(|e| {
            self.fail(
                Some(pattern.span()),
                format!("invalid regex in {place}: {e}"),
            )
        })
    }

    #[allow(clippy::too_many_lines)]
    fn rule(&self, raw: Spanned<RawRule>, seen: &mut HashSet<String>) -> Result<Rule, LoadError> {
        let rule_span = raw.span();
        let raw = raw.into_inner();

        let name = self.id(raw.id.as_ref(), &rule_span, "rule", seen)?;
        let at = |field: &str| format!("rule `{name}`: {field}");

        let kind = raw
            .kind
            .as_ref()
            .ok_or_else(|| self.fail(Some(rule_span.clone()), at("missing `kind`")))?;
        let severity = self.severity(raw.severity.as_ref(), &rule_span, &at)?;
        let message = raw
            .message
            .as_ref()
            .ok_or_else(|| self.fail(Some(rule_span.clone()), at("missing `message`")))?
            .get_ref()
            .clone();
        let suggestion = raw.suggestion.as_ref().map(|s| s.get_ref().clone());

        let allowed: &[&str] = match kind.get_ref().as_str() {
            "phrase" => &["registers", "phrases", "allow_when"],
            "regex" => &["registers", "pattern"],
            "oxford_comma" => &["registers", "max_item_words"],
            "sentence_limit" => &["max_sentences"],
            "file_links" => &[],
            other => {
                return Err(self.fail(
                    Some(kind.span()),
                    at(&format!("unknown kind `{other}`; expected one of {KINDS}")),
                ));
            }
        };
        let present = [
            ("registers", raw.registers.as_ref().map(Spanned::span)),
            ("phrases", raw.phrases.as_ref().map(Spanned::span)),
            ("allow_when", raw.allow_when.as_ref().map(Spanned::span)),
            ("pattern", raw.pattern.as_ref().map(Spanned::span)),
            (
                "max_item_words",
                raw.max_item_words.as_ref().map(Spanned::span),
            ),
            (
                "max_sentences",
                raw.max_sentences.as_ref().map(Spanned::span),
            ),
        ];
        for (field, span) in present {
            if let Some(span) = span
                && !allowed.contains(&field)
            {
                return Err(self.fail(
                    Some(span),
                    at(&format!(
                        "`{field}` is not valid for kind `{}`",
                        kind.get_ref()
                    )),
                ));
            }
        }

        let default_registers = match kind.get_ref().as_str() {
            "sentence_limit" => vec![Register::Comment],
            "file_links" => vec![Register::LongForm],
            _ => vec![Register::LongForm, Register::Comment],
        };
        let registers = self.registers(raw.registers.as_ref(), default_registers, &at)?;

        let kind = match kind.get_ref().as_str() {
            "phrase" => {
                let list = raw
                    .phrases
                    .as_ref()
                    .filter(|p| !p.get_ref().is_empty())
                    .ok_or_else(|| {
                        self.fail(Some(rule_span.clone()), at("needs a non-empty `phrases`"))
                    })?;
                let phrases = list
                    .get_ref()
                    .iter()
                    .map(|p| {
                        phrase_regex(p.get_ref())
                            .ok_or_else(|| self.fail(Some(p.span()), at("empty phrase")))
                    })
                    .collect::<Result<_, _>>()?;
                let allow_when = raw
                    .allow_when
                    .as_ref()
                    .map_or(&[][..], |a| a.get_ref().as_slice())
                    .iter()
                    .map(|p| self.regex(p, &at("`allow_when`")))
                    .collect::<Result<_, _>>()?;
                RuleKind::Phrase {
                    phrases,
                    allow_when,
                }
            }
            "regex" => {
                let pattern = raw
                    .pattern
                    .as_ref()
                    .ok_or_else(|| self.fail(Some(rule_span.clone()), at("needs a `pattern`")))?;
                RuleKind::Regex {
                    pattern: self.regex(pattern, &at("`pattern`"))?,
                }
            }
            "oxford_comma" => RuleKind::OxfordComma {
                max_item_words: match &raw.max_item_words {
                    None => DEFAULT_MAX_ITEM_WORDS,
                    Some(v) => self.at_least_one(v, &at("`max_item_words`"))?,
                },
            },
            "sentence_limit" => {
                let limits = raw.max_sentences.as_ref().ok_or_else(|| {
                    self.fail(Some(rule_span.clone()), at("needs `max_sentences`"))
                })?;
                let field = |v: &Option<Spanned<i64>>, level: &str| {
                    v.as_ref()
                        .map(|v| self.at_least_one(v, &at(&format!("`max_sentences.{level}`"))))
                        .transpose()
                };
                let parsed = SentenceLimits {
                    line: field(&limits.get_ref().line, "line")?,
                    item: field(&limits.get_ref().item, "item")?,
                    module: field(&limits.get_ref().module, "module")?,
                };
                if parsed == SentenceLimits::default() {
                    return Err(self.fail(
                        Some(limits.span()),
                        at("`max_sentences` needs at least one of line, item or module"),
                    ));
                }
                RuleKind::SentenceLimit { limits: parsed }
            }
            _ => RuleKind::FileLinks,
        };

        Ok(Rule {
            id: name,
            severity,
            registers,
            message,
            suggestion,
            kind,
        })
    }

    fn at_least_one(&self, v: &Spanned<i64>, what: &str) -> Result<usize, LoadError> {
        usize::try_from(*v.get_ref())
            .ok()
            .filter(|n| *n >= 1)
            .ok_or_else(|| self.fail(Some(v.span()), format!("{what} must be at least 1")))
    }
}

fn glob(pattern: &str) -> Result<globset::Glob, globset::Error> {
    GlobBuilder::new(pattern).literal_separator(true).build()
}

fn expand_home(pattern: &str, home: &Path) -> String {
    match pattern.strip_prefix("~/") {
        Some(rest) => format!("{}/{rest}", home.display()),
        None if pattern == "~" => home.display().to_string(),
        None => pattern.to_owned(),
    }
}

#[allow(clippy::naive_bytecount)]
fn line_of(src: &str, offset: usize) -> usize {
    src.as_bytes()[..offset.min(src.len())]
        .iter()
        .filter(|b| **b == b'\n')
        .count()
        + 1
}

/// Case-insensitive, whole-word where the phrase starts or ends on a word character, with any
/// whitespace run in the prose matching the phrase's spaces.
fn phrase_regex(phrase: &str) -> Option<Regex> {
    let words: Vec<String> = phrase.split_whitespace().map(regex::escape).collect();
    let first = phrase.trim_start().chars().next()?;
    let last = phrase.trim_end().chars().next_back()?;
    let boundary = |c: char| if is_word(c) { r"\b" } else { "" };
    let pattern = format!(
        "(?i){}{}{}",
        boundary(first),
        words.join(r"\s+"),
        boundary(last)
    );
    Some(Regex::new(&pattern).expect("an escaped phrase is a valid regex"))
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests;
