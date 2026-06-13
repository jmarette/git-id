use std::path::{Component, Path, PathBuf};

use anyhow::{Result, bail};

use crate::env::Env;

/// A directory normalized to the exact form stored in `gitdir:` patterns:
/// absolute, symlink-resolved as far as possible, with a trailing `/`.
///
/// Git matches `includeIf "gitdir:..."` patterns against the *realpath* of a
/// repository's `.git` directory first, so storing anything but the canonical
/// path makes routing silently fail (classic case: `/tmp` -> `/private/tmp`
/// on macOS).
pub struct NormalizedDir {
    /// The resolved directory, without trailing slash.
    pub path: PathBuf,
    /// `path` as a string with a trailing `/`, ready for a `gitdir:` pattern.
    pub gitdir: String,
    /// Whether the directory existed (and could be fully canonicalized).
    pub existed: bool,
}

pub fn normalize_dir(input: &Path, env: &Env) -> Result<NormalizedDir> {
    let Some(input_str) = input.to_str() else {
        bail!("path is not valid UTF-8: `{}`", input.display());
    };
    if input_str.is_empty() {
        bail!("path is empty");
    }
    let expanded = expand_tilde(input_str, &env.home);
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        env.cwd.join(expanded)
    };
    let (resolved, existed) = canonicalize_anchored(&absolute);
    if existed && !resolved.is_dir() {
        bail!("`{}` is not a directory", resolved.display());
    }
    let Some(resolved_str) = resolved.to_str() else {
        bail!("resolved path is not valid UTF-8: `{}`", resolved.display());
    };
    Ok(NormalizedDir {
        gitdir: ensure_trailing_slash(to_git_path(resolved_str)),
        path: resolved,
        existed,
    })
}

/// Expand a leading `~` or `~/...` to the home directory.
/// `~user` forms are not supported and returned unchanged.
pub fn expand_tilde(input: &str, home: &Path) -> PathBuf {
    if input == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = input.strip_prefix("~/") {
        return home.join(rest);
    }
    PathBuf::from(input)
}

/// Canonicalize `path` as far as the filesystem allows: every component that
/// exists is resolved through `fs::canonicalize` (symlinks, `.`, `..`, on-disk
/// casing), and the non-existing tail is appended with `.`/`..` cleaned
/// lexically against the already-canonical prefix.
///
/// Returns the resolved path and whether the full path existed.
pub fn canonicalize_anchored(path: &Path) -> (PathBuf, bool) {
    if let Ok(real) = std::fs::canonicalize(path) {
        return (real, true);
    }
    let mut resolved = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::RootDir | Component::Prefix(_) => resolved.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                // Resolve what we have so far before popping, so `..` from
                // inside a symlinked directory lands in the real parent,
                // matching what git/getcwd would see.
                if let Ok(real) = std::fs::canonicalize(&resolved) {
                    resolved = real;
                }
                resolved.pop();
            }
            Component::Normal(name) => {
                resolved.push(name);
                if let Ok(real) = std::fs::canonicalize(&resolved) {
                    resolved = real;
                }
            }
        }
    }
    (resolved, false)
}

pub fn ensure_trailing_slash(mut s: String) -> String {
    if !s.ends_with('/') {
        s.push('/');
    }
    s
}

/// Convert a resolved absolute path string into the form Git uses on this
/// platform for `gitdir:` patterns and config path values.
///
/// On Windows that means stripping the extended-length `\\?\` prefix that
/// `fs::canonicalize` emits and switching to forward slashes — the separator
/// Git matches `includeIf "gitdir:..."` against and writes in its own config.
/// On Unix it is the identity: there a `\` is a legitimate filename byte and
/// must be preserved. Only the *selection* is platform-gated; both helpers are
/// compiled and unit-tested on every host.
pub(crate) fn to_git_path(s: &str) -> String {
    if cfg!(windows) {
        git_path_windows(s)
    } else {
        s.to_string()
    }
}

fn git_path_windows(s: &str) -> String {
    // `\\?\UNC\server\share` -> `\\server\share`; `\\?\C:\dir` -> `C:\dir`.
    let stripped = if let Some(rest) = s.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{rest}")
    } else if let Some(rest) = s.strip_prefix("\\\\?\\") {
        rest.to_string()
    } else {
        s.to_string()
    };
    stripped.replace('\\', "/")
}

/// `*`, `?` and `[` are live wildmatch metacharacters inside `gitdir:`
/// patterns; a directory path containing them cannot be routed safely.
pub fn contains_glob_meta(s: &str) -> bool {
    s.bytes().any(|b| matches!(b, b'*' | b'?' | b'['))
}

/// Abbreviate the home directory as `~` for human-facing output only
/// (stored paths always stay absolute).
pub fn display_pretty(path: &str, home: &Path) -> String {
    // Normalize both sides to git-path form so the `~` abbreviation works
    // regardless of how the caller's path was spelled (identity on Unix).
    let path = to_git_path(path);
    let Some(home_str) = home.to_str() else {
        return path;
    };
    let home_str = to_git_path(home_str);
    if path == home_str {
        return "~".to_string();
    }
    let prefix = format!("{}/", home_str.trim_end_matches('/'));
    match path.strip_prefix(&prefix) {
        Some(rest) => format!("~/{rest}"),
        None => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::fs;

    fn env_at(home: &Path, cwd: &Path) -> Env {
        Env::new(home.to_path_buf(), cwd.to_path_buf(), None::<OsString>)
    }

    #[test]
    fn tilde_expansion() {
        let home = Path::new("/home/test");
        assert_eq!(expand_tilde("~", home), PathBuf::from("/home/test"));
        assert_eq!(
            expand_tilde("~/dev/x", home),
            PathBuf::from("/home/test/dev/x")
        );
        assert_eq!(expand_tilde("/abs", home), PathBuf::from("/abs"));
        assert_eq!(expand_tilde("rel/x", home), PathBuf::from("rel/x"));
        // `~user` is not supported: returned unchanged.
        assert_eq!(expand_tilde("~other/x", home), PathBuf::from("~other/x"));
    }

    #[test]
    fn trailing_slash_is_idempotent() {
        assert_eq!(ensure_trailing_slash("/a/b".into()), "/a/b/");
        assert_eq!(ensure_trailing_slash("/a/b/".into()), "/a/b/");
        assert_eq!(ensure_trailing_slash("/".into()), "/");
    }

    #[test]
    fn glob_meta_detection() {
        assert!(contains_glob_meta("/a/b*c/"));
        assert!(contains_glob_meta("/a/b?/"));
        assert!(contains_glob_meta("/a/[x]/"));
        assert!(!contains_glob_meta("/a/plain-dir_1/"));
    }

    #[test]
    #[cfg(unix)]
    fn canonicalize_existing_resolves_symlinks() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let real = root.join("real");
        fs::create_dir(&real).unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let (resolved, existed) = canonicalize_anchored(&link);
        assert!(existed);
        assert_eq!(resolved, real);
    }

    #[test]
    #[cfg(unix)]
    fn canonicalize_anchors_missing_tail() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let real = root.join("real");
        fs::create_dir(&real).unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        // The `missing/sub` tail does not exist: the existing ancestor (a
        // symlink) must still be resolved.
        let (resolved, existed) = canonicalize_anchored(&link.join("missing/sub"));
        assert!(!existed);
        assert_eq!(resolved, real.join("missing/sub"));
    }

    #[test]
    fn canonicalize_cleans_dots_in_missing_tail() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();

        let (resolved, existed) = canonicalize_anchored(&root.join("missing/./x/../y"));
        assert!(!existed);
        assert_eq!(resolved, root.join("missing/y"));
    }

    #[test]
    fn normalize_dir_makes_relative_absolute_with_slash() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let env = env_at(&root, &root);
        fs::create_dir(root.join("sub")).unwrap();

        let nd = normalize_dir(Path::new("sub"), &env).unwrap();
        assert!(nd.existed);
        // The gitdir is in git-path form (de-UNC'd, forward slashes), which is
        // the identity of `root` on Unix.
        let root_git = to_git_path(root.to_str().unwrap());
        assert_eq!(nd.gitdir, format!("{root_git}/sub/"));
        assert_eq!(nd.path, root.join("sub"));
    }

    #[test]
    fn normalize_dir_rejects_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let env = env_at(&root, &root);
        fs::write(root.join("file"), "x").unwrap();

        assert!(normalize_dir(Path::new("file"), &env).is_err());
    }

    #[test]
    fn display_pretty_abbreviates_home() {
        let home = Path::new("/home/test");
        assert_eq!(display_pretty("/home/test/dev/x/", home), "~/dev/x/");
        assert_eq!(display_pretty("/home/test", home), "~");
        assert_eq!(display_pretty("/elsewhere/x/", home), "/elsewhere/x/");
        assert_eq!(display_pretty("/home/testy/x/", home), "/home/testy/x/");
    }

    #[test]
    fn git_path_windows_strips_verbatim_prefix_and_uses_forward_slashes() {
        // Drive paths: the extended-length prefix is removed, separators flip.
        assert_eq!(git_path_windows("\\\\?\\C:\\Users\\x"), "C:/Users/x");
        assert_eq!(git_path_windows("C:\\Users\\x\\dev"), "C:/Users/x/dev");
        // UNC paths: `\\?\UNC\server\share` -> `//server/share`.
        assert_eq!(
            git_path_windows("\\\\?\\UNC\\server\\share\\x"),
            "//server/share/x"
        );
        assert_eq!(git_path_windows("\\\\server\\share"), "//server/share");
        // Already forward-slashed input is left alone.
        assert_eq!(git_path_windows("C:/Users/x"), "C:/Users/x");
    }

    #[test]
    fn doctor_canonical_comparison_uses_git_path_form() {
        // doctor recomputes a route's canonical path and compares it to the
        // stored gitdir. On Windows fs::canonicalize yields `\\?\C:\...`, so the
        // comparison must go through to_git_path (here: git_path_windows) or a
        // correct route is falsely flagged as non-canonical. This mirrors the
        // exact transformation doctor applies.
        assert_eq!(
            ensure_trailing_slash(git_path_windows("\\\\?\\C:\\Users\\jane\\dev")),
            "C:/Users/jane/dev/"
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn to_git_path_is_identity_on_unix() {
        // A backslash is a valid filename byte on Unix and must be preserved.
        assert_eq!(to_git_path("/home/jane/dev"), "/home/jane/dev");
        assert_eq!(to_git_path("/home/jane/we\\ird"), "/home/jane/we\\ird");
    }
}
