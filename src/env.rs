use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

/// Resolved process environment: where git-id reads and writes its files.
///
/// Built once at startup and passed by reference everywhere, so tests can
/// construct one directly without mutating the process environment.
pub struct Env {
    pub home: PathBuf,
    pub cwd: PathBuf,
    /// Base config directory: `$XDG_CONFIG_HOME` or `~/.config`.
    pub config_base: PathBuf,
    /// `<config_base>/git-id`
    pub config_dir: PathBuf,
    /// `<config_dir>/identities`
    pub identities_dir: PathBuf,
    /// `<config_dir>/routes.gitconfig`
    pub routes_file: PathBuf,
    /// An explicit `GIT_CONFIG_GLOBAL` value, when set — the file `git config
    /// --global` reads and writes instead of `~/.gitconfig`.
    pub git_config_global: Option<PathBuf>,
}

impl Env {
    pub fn from_process() -> Result<Self> {
        // Discover the home directory the way Git itself does, so our own
        // `global_config_write_target()` cannot diverge from what `git config`
        // resolves. Only the *selection* is platform-gated; both resolvers are
        // compiled and tested on every host.
        let home = if cfg!(windows) {
            resolve_home_windows(|key| std::env::var_os(key), |path| path.is_dir())?
        } else {
            resolve_home_unix(|key| std::env::var_os(key))?
        };
        ensure!(
            home.is_absolute(),
            "HOME must be an absolute path (got `{}`)",
            home.display()
        );
        let cwd = std::env::current_dir().context("cannot determine the current directory")?;
        let mut env = Self::new(home, cwd, std::env::var_os("XDG_CONFIG_HOME"));
        // An explicit GIT_CONFIG_GLOBAL relocates the global config; honor it so
        // our backup and messaging target the same file `git config --global`
        // writes to.
        env.git_config_global = std::env::var_os("GIT_CONFIG_GLOBAL")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        // gitconfig files and includeIf patterns are text: non-UTF-8 base paths
        // cannot be represented reliably, so refuse them up front.
        ensure!(
            env.config_dir.to_str().is_some() && env.home.to_str().is_some(),
            "HOME / XDG_CONFIG_HOME contain non-UTF-8 bytes, which git-id does not support"
        );
        Ok(env)
    }

    /// `xdg_config_home` follows the XDG spec: a relative (or empty) value is
    /// invalid and ignored, falling back to `~/.config`.
    pub fn new(home: PathBuf, cwd: PathBuf, xdg_config_home: Option<OsString>) -> Self {
        let config_base = xdg_config_home
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        let config_dir = config_base.join("git-id");
        Self {
            identities_dir: config_dir.join("identities"),
            routes_file: config_dir.join("routes.gitconfig"),
            config_dir,
            config_base,
            home,
            cwd,
            git_config_global: None,
        }
    }

    /// The file `git config --global` will write to, mirroring git's rules:
    /// an explicit `GIT_CONFIG_GLOBAL` wins; otherwise `~/.gitconfig`, unless
    /// it does not exist while `$XDG_CONFIG_HOME/git/config` does.
    pub fn global_config_write_target(&self) -> (PathBuf, bool) {
        if let Some(path) = &self.git_config_global {
            let exists = path.exists();
            return (path.clone(), exists);
        }
        let gitconfig = self.home.join(".gitconfig");
        if gitconfig.exists() {
            return (gitconfig, true);
        }
        let xdg = self.config_base.join("git").join("config");
        if xdg.exists() {
            (xdg, true)
        } else {
            (gitconfig, false)
        }
    }

    /// Base data directory: `$XDG_DATA_HOME` (when absolute) or
    /// `~/.local/share`. Read from the process, like git's XDG handling, so
    /// callers that write user data (shell completions, the man page) agree on
    /// where it goes.
    pub fn data_base(&self) -> PathBuf {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| self.home.join(".local/share"))
    }
}

/// Resolve the home directory on Unix: `HOME`, which must be set and non-empty.
/// Absoluteness is enforced by the caller, identically on every platform.
fn resolve_home_unix(lookup: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf> {
    lookup("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .context("HOME is not set; git-id needs it to locate your configuration")
}

/// Resolve the home directory on Windows, replicating Git for Windows
/// (`setup_windows_environment` in `compat/mingw.c`):
///
/// 1. `HOME`, if set and non-empty.
/// 2. else `HOMEDRIVE` + `HOMEPATH` concatenated — but only if the result is an
///    existing directory. This `is_dir` guard is how Git rejects the broken
///    `runas`/service case where `HOMEPATH` points into `\WINDOWS\system32`;
///    when the join is not a directory, it falls through to `USERPROFILE`.
/// 3. else `USERPROFILE`, if set and non-empty.
/// 4. else error.
///
/// The env lookup and the directory check are injected as closures so the
/// logic stays pure and unit-testable on any host (CI here runs on macOS).
fn resolve_home_windows(
    lookup: impl Fn(&str) -> Option<OsString>,
    is_dir: impl Fn(&Path) -> bool,
) -> Result<PathBuf> {
    if let Some(home) = lookup("HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(home));
    }
    if let (Some(drive), Some(path)) = (lookup("HOMEDRIVE"), lookup("HOMEPATH")) {
        // Plain concatenation, exactly like Git's strbuf: "C:" + "\Users\x".
        let mut joined = drive;
        joined.push(&path);
        let candidate = PathBuf::from(joined);
        if is_dir(&candidate) {
            return Ok(candidate);
        }
    }
    if let Some(profile) = lookup("USERPROFILE").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(profile));
    }
    bail!(
        "cannot locate your home directory: none of HOME, HOMEDRIVE+HOMEPATH, or USERPROFILE is set"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn home() -> PathBuf {
        PathBuf::from("/home/test")
    }

    #[test]
    fn default_config_base_is_dot_config() {
        let env = Env::new(home(), home(), None);
        assert_eq!(env.config_base, PathBuf::from("/home/test/.config"));
        assert_eq!(env.config_dir, PathBuf::from("/home/test/.config/git-id"));
        assert_eq!(
            env.routes_file,
            PathBuf::from("/home/test/.config/git-id/routes.gitconfig")
        );
        assert_eq!(
            env.identities_dir,
            PathBuf::from("/home/test/.config/git-id/identities")
        );
    }

    #[test]
    fn absolute_xdg_config_home_is_used() {
        // Use a path that is absolute on the test platform (a drive-rooted
        // path on Windows, where `/custom/xdg` would not be).
        let xdg = if cfg!(windows) {
            "C:\\custom\\xdg"
        } else {
            "/custom/xdg"
        };
        let env = Env::new(home(), home(), Some(OsString::from(xdg)));
        assert_eq!(env.config_dir, PathBuf::from(xdg).join("git-id"));
    }

    #[test]
    fn relative_or_empty_xdg_config_home_is_ignored() {
        let env = Env::new(home(), home(), Some(OsString::from("relative/path")));
        assert_eq!(env.config_base, PathBuf::from("/home/test/.config"));
        let env = Env::new(home(), home(), Some(OsString::from("")));
        assert_eq!(env.config_base, PathBuf::from("/home/test/.config"));
    }

    #[test]
    fn git_config_global_override_is_the_write_target() {
        let tmp = tempfile::TempDir::new().unwrap();
        let custom = tmp.path().join("custom.gitconfig");
        std::fs::write(&custom, "").unwrap();
        let mut env = Env::new(home(), home(), None);
        env.git_config_global = Some(custom.clone());
        let (target, exists) = env.global_config_write_target();
        assert_eq!(target, custom);
        assert!(exists);
    }

    #[test]
    fn without_override_falls_back_to_home_gitconfig() {
        // A home that does not exist: neither ~/.gitconfig nor the XDG file is
        // present, so the target is ~/.gitconfig reported as not-yet-existing.
        let env = Env::new(PathBuf::from("/no-such-home-xyz"), home(), None);
        let (target, exists) = env.global_config_write_target();
        assert_eq!(target, PathBuf::from("/no-such-home-xyz/.gitconfig"));
        assert!(!exists);
    }

    /// Build a deterministic env lookup from a fixed set of pairs.
    fn lookup_from(pairs: &[(&'static str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), OsString::from(*v)))
            .collect();
        move |key| map.get(key).cloned()
    }

    /// Build a deterministic `is_dir` from a fixed set of "existing" directories.
    fn dirs_from(dirs: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |path| dirs.iter().any(|d| Path::new(d) == path)
    }

    #[test]
    fn unix_uses_home_when_set() {
        let home = resolve_home_unix(lookup_from(&[("HOME", "/home/jane")])).unwrap();
        assert_eq!(home, PathBuf::from("/home/jane"));
    }

    #[test]
    fn unix_errors_when_home_missing_or_empty() {
        assert!(resolve_home_unix(lookup_from(&[])).is_err());
        assert!(resolve_home_unix(lookup_from(&[("HOME", "")])).is_err());
    }

    #[test]
    fn unix_relative_home_passes_through_but_fails_the_absolute_gate() {
        // The resolver returns the value verbatim; `from_process` rejects it
        // through the shared `is_absolute` check.
        let home = resolve_home_unix(lookup_from(&[("HOME", "relative/home")])).unwrap();
        assert!(
            !home.is_absolute(),
            "a relative HOME must fail the absolute gate"
        );
    }

    #[test]
    fn windows_home_takes_precedence() {
        let home = resolve_home_windows(
            lookup_from(&[
                ("HOME", "C:\\Home"),
                ("HOMEDRIVE", "C:"),
                ("HOMEPATH", "\\Users\\x"),
                ("USERPROFILE", "C:\\Users\\prof"),
            ]),
            dirs_from(&["C:\\Users\\x"]),
        )
        .unwrap();
        assert_eq!(home, PathBuf::from("C:\\Home"));
    }

    #[test]
    fn windows_joins_homedrive_homepath_when_it_is_a_directory() {
        let home = resolve_home_windows(
            lookup_from(&[("HOMEDRIVE", "C:"), ("HOMEPATH", "\\Users\\x")]),
            dirs_from(&["C:\\Users\\x"]),
        )
        .unwrap();
        assert_eq!(home, PathBuf::from("C:\\Users\\x"));
    }

    #[test]
    fn windows_falls_back_to_userprofile_when_join_is_not_a_directory() {
        // The classic runas/service case: HOMEPATH points somewhere bogus, so
        // the join is not a directory and USERPROFILE wins.
        let home = resolve_home_windows(
            lookup_from(&[
                ("HOMEDRIVE", "C:"),
                ("HOMEPATH", "\\WINDOWS\\system32"),
                ("USERPROFILE", "C:\\Users\\prof"),
            ]),
            dirs_from(&[]),
        )
        .unwrap();
        assert_eq!(home, PathBuf::from("C:\\Users\\prof"));
    }

    #[test]
    fn windows_uses_userprofile_when_only_it_is_set() {
        let home = resolve_home_windows(
            lookup_from(&[("USERPROFILE", "C:\\Users\\prof")]),
            dirs_from(&[]),
        )
        .unwrap();
        assert_eq!(home, PathBuf::from("C:\\Users\\prof"));
    }

    #[test]
    fn windows_errors_when_nothing_is_set() {
        assert!(resolve_home_windows(lookup_from(&[]), dirs_from(&[])).is_err());
    }
}
