//! A stand-in `claude` for tests: a shell script that records how it was called and answers
//! with canned output, so judge tests never touch the network.

use std::fmt::Write as _;
use std::path::PathBuf;

use serde_json::{Value, json};
use tempfile::TempDir;

pub(crate) struct FakeClaude {
    dir: TempDir,
    pub path: PathBuf,
}

/// The CLI's success envelope around `findings`.
pub(crate) fn success(findings: &Value) -> String {
    json!({
        "type": "result",
        "subtype": "success",
        "is_error": false,
        "result": json!({"findings": findings}).to_string(),
        "structured_output": {"findings": findings},
    })
    .to_string()
}

impl FakeClaude {
    /// Answers every call with `stdout` after `sleep` seconds, exiting with `exit`.
    pub(crate) fn new(stdout: &str, exit: i32, sleep: f32) -> Self {
        Self::routed(&[], stdout, exit, sleep)
    }

    pub(crate) fn answering(findings: &Value) -> Self {
        Self::new(&success(findings), 0, 0.0)
    }

    /// Answers with the first route whose marker appears anywhere in the arguments (the rubric
    /// rides in the system prompt), falling back to `default`.
    pub(crate) fn routed(routes: &[(&str, String)], default: &str, exit: i32, sleep: f32) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut cases = String::new();
        for (i, (marker, stdout)) in routes.iter().enumerate() {
            std::fs::write(dir.path().join(format!("route{i}")), stdout).unwrap();
            let _ = writeln!(cases, "  *{marker}*) out=\"$d/route{i}\" ;;");
        }
        std::fs::write(dir.path().join("stdout"), default).unwrap();
        std::fs::write(dir.path().join("stderr"), "fake stderr line\n").unwrap();
        let script = format!(
            "#!/bin/sh\n\
             d=\"$(dirname \"$0\")\"\n\
             for a in \"$@\"; do printf '%s\\0' \"$a\"; done > \"$d/argv\"\n\
             env > \"$d/env\"\n\
             cat > \"$d/stdin\"\n\
             echo call >> \"$d/calls\"\n\
             out=\"$d/stdout\"\n\
             case \"$*\" in\n{cases}esac\n\
             sleep {sleep}\n\
             cat \"$out\"\n\
             cat \"$d/stderr\" >&2\n\
             exit {exit}\n"
        );
        let path = dir.path().join("claude");
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self { dir, path }
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.path().join(name)).unwrap_or_default()
    }

    pub(crate) fn argv(&self) -> Vec<String> {
        let raw = self.read("argv");
        raw.split('\0')
            .take(raw.matches('\0').count())
            .map(str::to_owned)
            .collect()
    }

    pub(crate) fn env(&self) -> String {
        self.read("env")
    }

    pub(crate) fn stdin(&self) -> String {
        self.read("stdin")
    }

    pub(crate) fn calls(&self) -> usize {
        self.read("calls").lines().count()
    }

    pub(crate) fn command(&self) -> String {
        self.path.display().to_string()
    }
}
