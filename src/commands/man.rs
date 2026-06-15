use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::CommandFactory;
use clap_mangen::Man;

use crate::cli::Cli;
use crate::env::Env;
use crate::store;

/// File name of the installed man page (section 1).
const MAN_FILENAME: &str = "git-id.1";

/// Render the roff man page for the root command, from the same clap definition
/// that powers `--help` and the shell completions — so it can never drift.
fn render() -> Result<String> {
    let mut buf = Vec::new();
    Man::new(Cli::command())
        .render(&mut buf)
        .context("rendering the man page")?;
    String::from_utf8(buf).context("clap_mangen produced non-UTF-8 output")
}

/// Print the man page (roff) to stdout. Needs no environment (no HOME), so it
/// stays usable in minimal build/packaging setups. This is the idiomatic
/// primitive a packager installs — e.g. a Homebrew formula doing
/// `(man1/"git-id.1").write \`git-id man\``, as ripgrep's does.
pub fn print() -> Result<ExitCode> {
    io::stdout().write_all(render()?.as_bytes())?;
    Ok(ExitCode::SUCCESS)
}

/// Where the man page should live so `man` — and thus `git id --help`, which
/// git rewrites to `man git-id` — finds it without the user touching MANPATH.
///
/// Both macOS `man` and Linux man-db derive part of their search path from
/// `$PATH`, mapping a `…/bin` entry to its sibling `…/share/man`; a page placed
/// next to the binary's prefix is therefore found automatically. Returns `None`
/// on Windows, which has no man system. Only the platform *selection* is gated;
/// the path logic is pure and unit-tested on every host.
fn man_dir(exe: &Path, data_base: &Path) -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    // Homebrew runs from a Cellar and symlinks `<prefix>/share/man`; the
    // Cellar's own share/man is not on the manpath, so map back to <prefix>.
    if let Some(prefix) = homebrew_prefix(exe) {
        return Some(prefix.join("share").join("man").join("man1"));
    }
    // A binary in `<prefix>/bin` — cargo's `~/.cargo/bin`, a manual
    // `/usr/local/bin`, or the Homebrew symlink — maps to `<prefix>/share/man`.
    if let Some(bin) = exe.parent() {
        if bin.file_name().is_some_and(|n| n == "bin") {
            if let Some(prefix) = bin.parent() {
                return Some(prefix.join("share").join("man").join("man1"));
            }
        }
    }
    // Last resort: the XDG data dir. man-db finds `~/.local/share/man`; on
    // macOS it may not be on the default manpath (doctor flags discoverability).
    Some(data_base.join("man").join("man1"))
}

/// If `exe` lives under a Homebrew Cellar
/// (`<prefix>/Cellar/<pkg>/<ver>/bin/<exe>`), return `<prefix>`.
fn homebrew_prefix(exe: &Path) -> Option<PathBuf> {
    let mut prefix = PathBuf::new();
    for comp in exe.components() {
        if comp.as_os_str() == "Cellar" {
            return Some(prefix);
        }
        prefix.push(comp);
    }
    None
}

/// The path the man page is (or would be) installed at for this binary, or
/// `None` where man pages do not apply (Windows) or the executable is unknown.
pub fn installed_man_path(env: &Env) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(man_dir(&exe, &env.data_base())?.join(MAN_FILENAME))
}

/// Best-effort install of the man page. Returns the path written, or `None`
/// when there is no suitable target (Windows / unknown executable). Callers
/// must not fail on an `Err` here: the page is a convenience, not core setup.
pub fn install_man_page(env: &Env) -> Result<Option<PathBuf>> {
    let Some(path) = installed_man_path(env) else {
        return Ok(None);
    };
    store::atomic_write(&path, &render()?)?;
    Ok(Some(path))
}

/// Remove a man page previously installed by `install_man_page`, if present.
pub fn remove_man_page(env: &Env) -> Result<()> {
    if let Some(path) = installed_man_path(env) {
        if path.exists() {
            fs::remove_file(&path).with_context(|| format!("cannot remove {}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_emits_a_man_page_naming_the_binary_and_a_subcommand() {
        let page = render().unwrap();
        assert!(
            page.contains(".TH git-id"),
            "should be a roff page titled for the binary"
        );
        assert!(
            page.contains("SUBCOMMANDS"),
            "should carry a subcommands section"
        );
        assert!(page.contains("create"), "should list the create subcommand");
    }

    // The path logic is pure on every host; `man_dir` only gates the Windows
    // selection, so these expectations are written for the non-Windows branch.
    #[cfg(not(windows))]
    mod target {
        use super::*;

        #[test]
        fn cargo_bin_maps_to_sibling_share_man() {
            let dir = man_dir(
                Path::new("/home/jane/.cargo/bin/git-id"),
                Path::new("/home/jane/.local/share"),
            );
            assert_eq!(dir, Some(PathBuf::from("/home/jane/.cargo/share/man/man1")));
        }

        #[test]
        fn homebrew_cellar_maps_to_prefix_share_man() {
            let dir = man_dir(
                Path::new("/opt/homebrew/Cellar/git-id/0.3.0/bin/git-id"),
                Path::new("/home/jane/.local/share"),
            );
            assert_eq!(dir, Some(PathBuf::from("/opt/homebrew/share/man/man1")));
        }

        #[test]
        fn homebrew_symlink_in_bin_also_maps_to_prefix() {
            let dir = man_dir(
                Path::new("/opt/homebrew/bin/git-id"),
                Path::new("/home/jane/.local/share"),
            );
            assert_eq!(dir, Some(PathBuf::from("/opt/homebrew/share/man/man1")));
        }

        #[test]
        fn unknown_layout_falls_back_to_xdg_data_dir() {
            // e.g. the test binary at `target/debug/git-id` (parent not `bin`).
            let dir = man_dir(
                Path::new("/work/git-id/target/debug/git-id"),
                Path::new("/home/jane/.local/share"),
            );
            assert_eq!(dir, Some(PathBuf::from("/home/jane/.local/share/man/man1")));
        }
    }

    #[test]
    #[cfg(windows)]
    fn windows_has_no_man_target() {
        assert_eq!(
            man_dir(
                Path::new("C:\\Users\\jane\\.cargo\\bin\\git-id.exe"),
                Path::new("C:\\Users\\jane\\AppData")
            ),
            None
        );
    }
}
