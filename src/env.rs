use std::ffi::OsString;
use std::path::PathBuf;
use std::time::SystemTime;

/// Everything the process reads from its surroundings, injected so tests never touch the real home.
#[derive(Debug, Clone)]
pub struct Env {
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub xdg_config_home: Option<PathBuf>,
    pub xdg_state_home: Option<PathBuf>,
    /// `VOICE_ENFORCER_RULESET`
    pub ruleset_dir: Option<PathBuf>,
    pub now: SystemTime,
    /// The running binary, used by `install` to write an absolute hook command.
    pub exe: PathBuf,
}

impl Env {
    pub fn from_process() -> Self {
        Self::from_parts(
            |name| std::env::var_os(name),
            std::env::home_dir(),
            std::env::current_dir().ok(),
            std::env::current_exe().ok(),
            SystemTime::now(),
        )
    }

    fn from_parts(
        var: impl Fn(&str) -> Option<OsString>,
        home: Option<PathBuf>,
        cwd: Option<PathBuf>,
        exe: Option<PathBuf>,
        now: SystemTime,
    ) -> Self {
        let path_var = |name| var(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home = home.unwrap_or_else(|| PathBuf::from("/"));
        Self {
            cwd: cwd.unwrap_or_else(|| home.clone()),
            home,
            xdg_config_home: path_var("XDG_CONFIG_HOME"),
            xdg_state_home: path_var("XDG_STATE_HOME"),
            ruleset_dir: path_var("VOICE_ENFORCER_RULESET"),
            now,
            exe: exe.unwrap_or_else(|| PathBuf::from("voice-enforcer")),
        }
    }

    pub fn config_dir(&self) -> PathBuf {
        self.xdg_config_home
            .clone()
            .unwrap_or_else(|| self.home.join(".config"))
            .join("voice-enforcer")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.xdg_state_home
            .clone()
            .unwrap_or_else(|| self.home.join(".local/state"))
            .join("voice-enforcer")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_process_fills_paths() {
        let env = Env::from_process();
        assert!(env.home.is_absolute());
        assert!(env.cwd.is_absolute());
    }

    #[test]
    fn missing_parts_fall_back() {
        let env = Env::from_parts(
            |name| (name == "XDG_STATE_HOME").then(OsString::new),
            None,
            None,
            None,
            SystemTime::UNIX_EPOCH,
        );
        assert_eq!(env.home, PathBuf::from("/"));
        assert_eq!(env.cwd, PathBuf::from("/"));
        assert_eq!(env.exe, PathBuf::from("voice-enforcer"));
        assert_eq!(env.xdg_state_home, None, "empty vars count as unset");
    }

    #[test]
    fn vars_are_read() {
        let env = Env::from_parts(
            |name| Some(OsString::from(format!("/{name}"))),
            Some(PathBuf::from("/h")),
            Some(PathBuf::from("/c")),
            Some(PathBuf::from("/e")),
            SystemTime::UNIX_EPOCH,
        );
        assert_eq!(
            env.ruleset_dir,
            Some(PathBuf::from("/VOICE_ENFORCER_RULESET"))
        );
        assert_eq!(env.xdg_config_home, Some(PathBuf::from("/XDG_CONFIG_HOME")));
        assert_eq!(env.cwd, PathBuf::from("/c"));
        assert_eq!(env.exe, PathBuf::from("/e"));
    }

    #[test]
    fn dirs_prefer_xdg() {
        let mut env = Env::from_process();
        env.home = PathBuf::from("/h");
        env.xdg_state_home = None;
        env.xdg_config_home = None;
        assert_eq!(
            env.state_dir(),
            PathBuf::from("/h/.local/state/voice-enforcer")
        );
        assert_eq!(env.config_dir(), PathBuf::from("/h/.config/voice-enforcer"));
        env.xdg_state_home = Some(PathBuf::from("/x"));
        env.xdg_config_home = Some(PathBuf::from("/y"));
        assert_eq!(env.state_dir(), PathBuf::from("/x/voice-enforcer"));
        assert_eq!(env.config_dir(), PathBuf::from("/y/voice-enforcer"));
    }
}
