//! Verdicts keyed on everything that could change them, so identical input never pays twice.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

use crate::judges::runner::{HARNESS_VERSION, Verdict};
use crate::ruleset::{Judge, JudgeSettings};

const UNUSED_FOR: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const PRUNE_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug)]
pub(crate) struct Cache {
    dir: PathBuf,
    now: SystemTime,
}

impl Cache {
    /// `root` is the tool's cache dir; verdicts live in `root/verdicts`.
    pub(crate) fn new(root: &Path, now: SystemTime) -> Self {
        Self {
            dir: root.join("verdicts"),
            now,
        }
    }

    pub(crate) fn key(judge: &Judge, settings: &JudgeSettings, input: &str) -> String {
        Self::key_at(HARNESS_VERSION, judge, settings, input)
    }

    fn key_at(harness: u32, judge: &Judge, settings: &JudgeSettings, input: &str) -> String {
        let mut hash = Sha256::new();
        for part in [
            harness.to_string().as_str(),
            &judge.id,
            &judge.rubric,
            &settings.model,
            &settings.effort,
            input,
        ] {
            hash.update(part.as_bytes());
            hash.update([0]);
        }
        hash.finalize()
            .iter()
            .fold(String::with_capacity(64), |mut hex, b| {
                let _ = write!(hex, "{b:02x}");
                hex
            })
    }

    fn entry(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    /// A hit refreshes the entry's mtime, so pruning removes only entries nobody has used.
    pub(crate) fn get(&self, key: &str) -> Option<Verdict> {
        let path = self.entry(key);
        let verdict = serde_json::from_str(&fs::read_to_string(&path).ok()?).ok()?;
        if let Ok(file) = fs::File::options().write(true).open(&path) {
            let _ = file.set_modified(self.now);
        }
        Some(verdict)
    }

    /// Best effort: a cache that can't be written just means the next call runs again.
    pub(crate) fn put(&self, key: &str, verdict: &Verdict) {
        if fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let tmp = self.dir.join(format!(".{key}.tmp-{}", std::process::id()));
        let json = serde_json::to_string(verdict).expect("a verdict always serializes");
        if fs::write(&tmp, json).is_ok() && fs::rename(&tmp, self.entry(key)).is_err() {
            let _ = fs::remove_file(&tmp);
        }
    }

    /// Deletes entries unused for 30 days, at most once a day (a stamp file records the last run).
    pub(crate) fn prune(&self) {
        let stamp = self.dir.join(".last-prune");
        let recent = fs::metadata(&stamp)
            .and_then(|m| m.modified())
            .is_ok_and(|t| self.now.duration_since(t).unwrap_or_default() < PRUNE_EVERY);
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        if recent {
            return;
        }
        for entry in entries.flatten() {
            let stale = entry
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| self.now.duration_since(t).unwrap_or_default() > UNUSED_FOR);
            if stale && entry.path() != stamp {
                let _ = fs::remove_file(entry.path());
            }
        }
        if fs::write(&stamp, "").is_ok()
            && let Ok(file) = fs::File::options().write(true).open(&stamp)
        {
            let _ = file.set_modified(self.now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::judges::runner::RawFinding;
    use crate::ruleset::Scope;
    use crate::types::{Register, Severity};

    fn judge(rubric: &str) -> Judge {
        Judge {
            id: "j".into(),
            severity: Severity::Warn,
            scope: Scope::Diff,
            registers: vec![Register::LongForm],
            rubric: rubric.into(),
            excludes: crate::ruleset::Excludes::default(),
            model: None,
            effort: None,
        }
    }

    fn verdict() -> Verdict {
        Verdict {
            findings: vec![RawFinding {
                quote: "q".into(),
                explanation: "e".into(),
                suggestion: None,
                violates: true,
            }],
        }
    }

    #[test]
    fn keys_change_with_every_input() {
        let s = JudgeSettings::default();
        let base = Cache::key(&judge("r"), &s, "in");
        assert_eq!(base.len(), 64);
        assert_eq!(base, Cache::key(&judge("r"), &s, "in"));
        assert_ne!(base, Cache::key(&judge("r2"), &s, "in"));
        assert_ne!(base, Cache::key(&judge("r"), &s, "in2"));
        let other = JudgeSettings {
            model: "sonnet".into(),
            ..JudgeSettings::default()
        };
        assert_ne!(base, Cache::key(&judge("r"), &other, "in"));
        let mut renamed = judge("r");
        renamed.id = "k".into();
        assert_ne!(base, Cache::key(&renamed, &s, "in"));
        assert_ne!(
            base,
            Cache::key_at(HARNESS_VERSION - 1, &judge("r"), &s, "in"),
            "a harness bump retires every cached verdict"
        );
    }

    #[test]
    fn put_then_get() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path(), SystemTime::now());
        assert_eq!(cache.get("abc"), None);
        cache.put("abc", &verdict());
        assert_eq!(cache.get("abc"), Some(verdict()));
    }

    #[test]
    fn corrupt_entry_is_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path(), SystemTime::now());
        fs::create_dir_all(dir.path().join("verdicts")).unwrap();
        fs::write(dir.path().join("verdicts/abc.json"), "{nope").unwrap();
        assert_eq!(cache.get("abc"), None);
    }

    #[test]
    fn unwritable_cache_is_best_effort() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("verdicts"), "a file, not a dir").unwrap();
        let cache = Cache::new(dir.path(), SystemTime::now());
        cache.put("abc", &verdict());
        assert_eq!(cache.get("abc"), None);
        cache.prune();
    }

    #[test]
    fn failed_rename_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path(), SystemTime::now());
        fs::create_dir_all(dir.path().join("verdicts/abc.json/inside")).unwrap();
        cache.put("abc", &verdict());
        let leftovers = fs::read_dir(dir.path().join("verdicts"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn prune_removes_unused_entries_once_a_day() {
        let dir = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        let cache = Cache::new(dir.path(), now);
        cache.put("old", &verdict());
        cache.put("used", &verdict());

        let later = now + UNUSED_FOR + Duration::from_secs(60);
        let reader = Cache::new(dir.path(), later);
        assert!(reader.get("used").is_some(), "a hit refreshes the entry");
        reader.prune();
        assert_eq!(reader.get("old"), None);
        assert!(reader.get("used").is_some());

        reader.put("fresh", &verdict());
        let file = fs::File::options()
            .write(true)
            .open(dir.path().join("verdicts/fresh.json"))
            .unwrap();
        file.set_modified(now).unwrap();
        Cache::new(dir.path(), later + Duration::from_secs(60)).prune();
        assert!(
            dir.path().join("verdicts/fresh.json").exists(),
            "pruned less than a day ago, so this run is skipped"
        );
    }

    #[test]
    fn prune_without_a_cache_dir_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        Cache::new(&dir.path().join("missing"), SystemTime::now()).prune();
    }
}
