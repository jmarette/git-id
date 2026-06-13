//! Thin wrappers around `git config` (and a couple of `git rev-parse` calls).
//!
//! git-id manipulates the files it owns as plain text, but leans on git
//! itself for everything else: surgical edits of identity fragments
//! (`git config --file`), the global include line (`git config --global`),
//! and resolving the *effective* identity of a directory (`git -C`).
//!
//! Relevant `git config` exit codes (from git-config(1)):
//! - 1: the key is absent -> mapped to `None` / empty
//! - 5: unsetting a key that does not exist -> treated as success
//!
//! Anything else unexpected is surfaced as an error with git's stderr.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, bail};

fn run(cmd: &mut Command) -> Result<Output> {
    cmd.output()
        .context("failed to run `git` — is git installed and on PATH?")
}

fn stdout_trimmed(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout)
        .trim_end_matches('\n')
        .to_string()
}

fn stderr_trimmed(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).trim().to_string()
}

fn check(out: &Output, what: &str) -> Result<()> {
    if out.status.success() {
        Ok(())
    } else {
        bail!("{what} failed: {}", stderr_trimmed(out));
    }
}

/// `git config --file <file> --get <key>` -> None when the key is absent.
pub fn get_file(file: &Path, key: &str) -> Result<Option<String>> {
    let out = run(Command::new("git")
        .arg("config")
        .arg("--file")
        .arg(file)
        .args(["--get", key]))?;
    match out.status.code() {
        Some(0) => Ok(Some(stdout_trimmed(&out))),
        Some(1) => Ok(None),
        _ => bail!(
            "`git config --file {} --get {key}` failed: {}",
            file.display(),
            stderr_trimmed(&out)
        ),
    }
}

/// Like [`get_file`] but normalized by git to a boolean.
pub fn get_file_bool(file: &Path, key: &str) -> Result<Option<bool>> {
    let out = run(Command::new("git")
        .arg("config")
        .arg("--file")
        .arg(file)
        .args(["--type=bool", "--get", key]))?;
    match out.status.code() {
        Some(0) => Ok(Some(stdout_trimmed(&out) == "true")),
        Some(1) => Ok(None),
        _ => bail!(
            "`git config --file {} --get {key}` failed: {}",
            file.display(),
            stderr_trimmed(&out)
        ),
    }
}

pub fn set_file(file: &Path, key: &str, value: &str) -> Result<()> {
    let out = run(Command::new("git")
        .arg("config")
        .arg("--file")
        .arg(file)
        .arg(key)
        .arg(value))?;
    check(
        &out,
        &format!("`git config --file {} {key}`", file.display()),
    )
}

/// Unset a key; missing key (exit code 5) is fine.
pub fn unset_file(file: &Path, key: &str) -> Result<()> {
    let out = run(Command::new("git")
        .arg("config")
        .arg("--file")
        .arg(file)
        .args(["--unset", key]))?;
    match out.status.code() {
        Some(0) | Some(5) => Ok(()),
        _ => bail!(
            "`git config --file {} --unset {key}` failed: {}",
            file.display(),
            stderr_trimmed(&out)
        ),
    }
}

/// `git config --global --get <key>` -> None when absent (or when no global
/// config file exists at all).
pub fn global_get(key: &str) -> Result<Option<String>> {
    let out = run(Command::new("git").args(["config", "--global", "--get", key]))?;
    match out.status.code() {
        Some(0) => Ok(Some(stdout_trimmed(&out))),
        Some(1) => Ok(None),
        _ => bail!(
            "`git config --global --get {key}` failed: {}",
            stderr_trimmed(&out)
        ),
    }
}

/// All values of a multi-valued global key; empty when absent.
pub fn global_get_all(key: &str) -> Result<Vec<String>> {
    let out = run(Command::new("git").args(["config", "--global", "--get-all", key]))?;
    match out.status.code() {
        Some(0) => Ok(stdout_trimmed(&out).lines().map(str::to_string).collect()),
        Some(1) => Ok(Vec::new()),
        _ => bail!(
            "`git config --global --get-all {key}` failed: {}",
            stderr_trimmed(&out)
        ),
    }
}

/// Append a value to a (multi-valued) global key.
pub fn global_add(key: &str, value: &str) -> Result<()> {
    let out = run(Command::new("git").args(["config", "--global", "--add", key, value]))?;
    check(&out, &format!("`git config --global --add {key}`"))
}

/// Set a single-valued global key.
pub fn global_set(key: &str, value: &str) -> Result<()> {
    let out = run(Command::new("git").args(["config", "--global", key, value]))?;
    check(&out, &format!("`git config --global {key}`"))
}

/// Unset a single-valued global key; a missing key (exit 5) is fine.
pub fn global_unset(key: &str) -> Result<()> {
    let out = run(Command::new("git").args(["config", "--global", "--unset", key]))?;
    match out.status.code() {
        Some(0) | Some(5) => Ok(()),
        _ => bail!(
            "`git config --global --unset {key}` failed: {}",
            stderr_trimmed(&out)
        ),
    }
}

/// Remove every global value of `key` matching `value_regex`; no match
/// (exit 5) is fine.
pub fn global_unset_all_matching(key: &str, value_regex: &str) -> Result<()> {
    let out =
        run(Command::new("git").args(["config", "--global", "--unset-all", key, value_regex]))?;
    match out.status.code() {
        Some(0) | Some(5) => Ok(()),
        _ => bail!(
            "`git config --global --unset-all {key}` failed: {}",
            stderr_trimmed(&out)
        ),
    }
}

/// Effective value of a key as git resolves it from inside `dir`
/// (includes, conditional includes, repo-local config — everything).
pub fn effective(dir: &Path, key: &str) -> Result<Option<String>> {
    let out = run(Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["config", "--get", key]))?;
    match out.status.code() {
        Some(0) => Ok(Some(stdout_trimmed(&out))),
        Some(1) => Ok(None),
        _ => bail!(
            "`git -C {} config --get {key}` failed: {}",
            dir.display(),
            stderr_trimmed(&out)
        ),
    }
}

/// Effective value plus the file it came from, e.g.
/// `("/home/u/.config/git-id/identities/work.gitconfig", "a@b.c")`.
pub fn effective_origin(dir: &Path, key: &str) -> Result<Option<(String, String)>> {
    let out = run(Command::new("git").arg("-C").arg(dir).args([
        "config",
        "--show-origin",
        "--get",
        key,
    ]))?;
    match out.status.code() {
        Some(0) => {
            let line = stdout_trimmed(&out);
            let Some((origin, value)) = line.split_once('\t') else {
                bail!("unexpected `git config --show-origin` output: `{line}`");
            };
            let origin = origin.strip_prefix("file:").unwrap_or(origin);
            Ok(Some((origin.to_string(), value.to_string())))
        }
        Some(1) => Ok(None),
        _ => bail!(
            "`git -C {} config --show-origin --get {key}` failed: {}",
            dir.display(),
            stderr_trimmed(&out)
        ),
    }
}

/// Absolute path of the `.git` directory governing `dir`, or None when `dir`
/// is not inside a git repository. This is the location git matches
/// `includeIf "gitdir:..."` patterns against.
pub fn absolute_git_dir(dir: &Path) -> Result<Option<PathBuf>> {
    let out = run(Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--absolute-git-dir"]))?;
    if out.status.success() {
        Ok(Some(PathBuf::from(stdout_trimmed(&out))))
    } else {
        Ok(None)
    }
}

/// Root of the working tree containing `dir`, or None outside a work tree.
pub fn show_toplevel(dir: &Path) -> Result<Option<PathBuf>> {
    let out = run(Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"]))?;
    if out.status.success() {
        Ok(Some(PathBuf::from(stdout_trimmed(&out))))
    } else {
        Ok(None)
    }
}

/// Parsed `git version` (e.g. (2, 50, 1)); tolerant of vendor suffixes.
pub fn git_version() -> Result<(u32, u32, u32)> {
    let out = run(Command::new("git").arg("--version"))?;
    check(&out, "`git --version`")?;
    let text = stdout_trimmed(&out);
    let version = text
        .split_whitespace()
        .find(|tok| tok.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .with_context(|| format!("cannot parse `{text}`"))?;
    let mut parts = version.split('.').map(|seg| {
        seg.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse::<u32>()
            .unwrap_or(0)
    });
    Ok((
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_roundtrip_get_set_unset() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("fragment.gitconfig");

        assert_eq!(get_file(&file, "user.email").unwrap(), None);
        set_file(&file, "user.email", "a@b.co").unwrap();
        set_file(&file, "user.name", "Jane Doe").unwrap();
        assert_eq!(
            get_file(&file, "user.email").unwrap().as_deref(),
            Some("a@b.co")
        );
        assert_eq!(
            get_file(&file, "user.name").unwrap().as_deref(),
            Some("Jane Doe")
        );

        // Unsetting a missing key is not an error (git exit code 5).
        unset_file(&file, "user.signingkey").unwrap();
        unset_file(&file, "user.email").unwrap();
        assert_eq!(get_file(&file, "user.email").unwrap(), None);
    }

    #[test]
    fn file_bool_normalization() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("fragment.gitconfig");

        assert_eq!(get_file_bool(&file, "commit.gpgsign").unwrap(), None);
        set_file(&file, "commit.gpgsign", "yes").unwrap();
        assert_eq!(get_file_bool(&file, "commit.gpgsign").unwrap(), Some(true));
        set_file(&file, "commit.gpgsign", "0").unwrap();
        assert_eq!(get_file_bool(&file, "commit.gpgsign").unwrap(), Some(false));
    }

    #[test]
    fn git_version_parses() {
        let (major, minor, _) = git_version().unwrap();
        assert!(major >= 2, "unexpectedly old git: {major}.{minor}");
    }
}
