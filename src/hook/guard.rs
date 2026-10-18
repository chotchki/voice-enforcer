//! Caps consecutive denies per (session, file) so one bad rule can't wedge a session.
//!
//! State is one small file per pair, so parallel hook processes in a session never share a file.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const PRUNE_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Verdict {
    Deny,
    /// The limit was reached: report the blocks as warnings instead.
    Tripped,
}

#[derive(Debug)]
pub(super) struct Guard {
    dir: PathBuf,
    file: PathBuf,
    max: u32,
}

impl Guard {
    pub(super) fn new(state_dir: &Path, session_id: &str, path: &Path, max: u32) -> Self {
        let dir = sessions_dir(state_dir).join(safe_session(session_id));
        let file = dir.join(format!(
            "{:016x}",
            fnv1a(path.as_os_str().as_encoded_bytes())
        ));
        Self { dir, file, max }
    }

    fn count(&self) -> u32 {
        fs::read_to_string(&self.file)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    }

    /// Called when an evaluation has blocking findings.
    pub(super) fn on_block(&self) -> Verdict {
        let count = self.count();
        if count >= self.max {
            return Verdict::Tripped;
        }
        self.store(count + 1);
        Verdict::Deny
    }

    /// Called when an evaluation has no blocking findings.
    pub(super) fn on_clean(&self) {
        if self.file.exists() {
            self.store(0);
        }
    }

    /// State is best effort: failing to save it just means the guard trips later or never.
    fn store(&self, count: u32) {
        if fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let name = self.file.file_name().unwrap_or_default().to_string_lossy();
        let tmp = self
            .dir
            .join(format!(".{name}.tmp-{}-{}", std::process::id(), unique()));
        if fs::write(&tmp, count.to_string()).is_ok() && fs::rename(&tmp, &self.file).is_err() {
            let _ = fs::remove_file(&tmp);
        }
    }
}

fn sessions_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("sessions")
}

/// Deletes session state untouched for 7 days.
pub(super) fn prune(state_dir: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(sessions_dir(state_dir)) else {
        return;
    };
    for entry in entries.flatten() {
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| now.duration_since(t).unwrap_or_default() > PRUNE_AFTER);
        if stale {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Session ids come from the payload, so anything outside `[A-Za-z0-9_-]` is hashed rather than
/// trusted as a path component.
fn safe_session(id: &str) -> String {
    if !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        id.to_owned()
    } else {
        format!("h{:016x}", fnv1a(id.as_bytes()))
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn unique() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denies_up_to_the_limit_then_trips() {
        let dir = tempfile::tempdir().unwrap();
        let g = Guard::new(dir.path(), "s1", Path::new("/r/a.md"), 3);
        assert_eq!(g.on_block(), Verdict::Deny);
        assert_eq!(g.on_block(), Verdict::Deny);
        assert_eq!(g.on_block(), Verdict::Deny);
        assert_eq!(g.on_block(), Verdict::Tripped);
        assert_eq!(g.on_block(), Verdict::Tripped, "stays tripped until clean");
    }

    #[test]
    fn clean_resets() {
        let dir = tempfile::tempdir().unwrap();
        let g = Guard::new(dir.path(), "s1", Path::new("/r/a.md"), 2);
        g.on_clean();
        assert!(
            !dir.path().join("sessions").exists(),
            "clean with no state writes nothing"
        );
        assert_eq!(g.on_block(), Verdict::Deny);
        assert_eq!(g.on_block(), Verdict::Deny);
        g.on_clean();
        assert_eq!(g.on_block(), Verdict::Deny);
    }

    #[test]
    fn sessions_and_files_are_independent() {
        let dir = tempfile::tempdir().unwrap();
        let a = Guard::new(dir.path(), "s1", Path::new("/r/a.md"), 1);
        assert_eq!(a.on_block(), Verdict::Deny);
        assert_eq!(a.on_block(), Verdict::Tripped);
        let other_session = Guard::new(dir.path(), "s2", Path::new("/r/a.md"), 1);
        assert_eq!(other_session.on_block(), Verdict::Deny);
        let other_file = Guard::new(dir.path(), "s1", Path::new("/r/b.md"), 1);
        assert_eq!(other_file.on_block(), Verdict::Deny);
    }

    #[test]
    fn parallel_writers_keep_their_own_counts() {
        let dir = tempfile::tempdir().unwrap();
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let state = dir.path().to_path_buf();
                std::thread::spawn(move || {
                    let g = Guard::new(&state, "s1", Path::new(&format!("/r/{i}.md")), 5);
                    for _ in 0..3 {
                        g.on_block();
                    }
                    g.count()
                })
            })
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap(), 3);
        }
    }

    #[test]
    fn hostile_session_ids_are_hashed() {
        assert_eq!(safe_session("abc-123_X"), "abc-123_X");
        for id in ["../../etc", "", "a/b", &"x".repeat(200)] {
            let s = safe_session(id);
            assert!(s.starts_with('h') && s.len() == 17, "{id:?} -> {s}");
        }
    }

    #[test]
    fn prune_removes_only_stale_sessions() {
        let dir = tempfile::tempdir().unwrap();
        Guard::new(dir.path(), "old", Path::new("/r/a.md"), 3).on_block();
        prune(dir.path(), SystemTime::now());
        assert!(dir.path().join("sessions/old").exists());
        prune(
            dir.path(),
            SystemTime::now() + PRUNE_AFTER + Duration::from_secs(60),
        );
        assert!(!dir.path().join("sessions/old").exists());
        prune(&dir.path().join("missing"), SystemTime::now());
    }

    #[test]
    fn failed_rename_cleans_up_its_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let g = Guard::new(dir.path(), "s1", Path::new("/r/a.md"), 1);
        fs::create_dir_all(&g.file).unwrap();
        assert_eq!(g.on_block(), Verdict::Deny);
        let leftovers: Vec<_> = fs::read_dir(&g.dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn unwritable_state_is_best_effort() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("sessions");
        fs::write(&blocker, "not a dir").unwrap();
        let g = Guard::new(dir.path(), "s1", Path::new("/r/a.md"), 1);
        assert_eq!(g.on_block(), Verdict::Deny);
        assert_eq!(g.on_block(), Verdict::Deny, "nothing saved, never trips");
    }
}
