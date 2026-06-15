use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{CommandFactory, ValueEnum};
use clap_complete::{Shell, generate};
use clap_complete_nushell::Nushell;

use crate::cli::{Cli, CompletionShell, CompletionsAction, CompletionsArgs};
use crate::env::Env;
use crate::paths::display_pretty;
use crate::store;

/// Render the completion script for `shell` as a UTF-8 string.
fn render(shell: CompletionShell) -> String {
    let mut cmd = Cli::command();
    let mut buf = Vec::new();
    match shell {
        CompletionShell::Bash => generate(Shell::Bash, &mut cmd, "git-id", &mut buf),
        CompletionShell::Elvish => generate(Shell::Elvish, &mut cmd, "git-id", &mut buf),
        CompletionShell::Fish => generate(Shell::Fish, &mut cmd, "git-id", &mut buf),
        CompletionShell::Nushell => generate(Nushell, &mut cmd, "git-id", &mut buf),
        CompletionShell::Powershell => generate(Shell::PowerShell, &mut cmd, "git-id", &mut buf),
        CompletionShell::Zsh => generate(Shell::Zsh, &mut cmd, "git-id", &mut buf),
    }
    String::from_utf8(buf).expect("clap_complete emits UTF-8")
}

fn shell_name(shell: CompletionShell) -> String {
    shell
        .to_possible_value()
        .expect("no CompletionShell variant is skipped")
        .get_name()
        .to_string()
}

/// Map a shell's executable path (typically `$SHELL`) to a known shell.
fn detect_shell(shell_path: Option<&str>) -> Option<CompletionShell> {
    let path = shell_path?;
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    Some(match name {
        "bash" => CompletionShell::Bash,
        "zsh" => CompletionShell::Zsh,
        "fish" => CompletionShell::Fish,
        "nu" | "nushell" => CompletionShell::Nushell,
        "elvish" => CompletionShell::Elvish,
        "pwsh" | "powershell" => CompletionShell::Powershell,
        _ => return None,
    })
}

/// Detect the shell from env vars the running shell exports to child processes.
///
/// `$SHELL` is the *login* shell (set by `/etc/passwd` and inherited); it does
/// not change when the user switches shells interactively. Shell-specific
/// version variables are exported by the running shell itself and are therefore
/// reliable for detecting the actual calling shell.
///
/// `getenv` is injectable for tests; production callers pass
/// `|v| std::env::var(v).ok()`.
fn detect_running_shell(getenv: impl Fn(&str) -> Option<String>) -> Option<CompletionShell> {
    // Each variable is exported by the named shell to its child processes.
    // Check most-specific first so nested shells (e.g. nu inside zsh) resolve
    // to the innermost one rather than an ancestor.
    for (var, shell) in [
        ("NU_VERSION", CompletionShell::Nushell),
        ("FISH_VERSION", CompletionShell::Fish),
    ] {
        if getenv(var).is_some() {
            return Some(shell);
        }
    }
    // Fall back to $SHELL for traditional POSIX-ish shells (bash, zsh, elvish,
    // powershell) that don't export a version variable to child processes.
    detect_shell(getenv("SHELL").as_deref())
}

/// The explicit shell, or one detected from the running shell's environment.
fn resolve_shell(explicit: Option<CompletionShell>) -> Result<CompletionShell> {
    if let Some(shell) = explicit {
        return Ok(shell);
    }
    if let Some(shell) = detect_running_shell(|v| std::env::var(v).ok()) {
        return Ok(shell);
    }
    bail!(
        "could not detect your shell; name it explicitly, e.g. \
         `git-id completions install nushell` (bash, zsh, fish, nushell, elvish, powershell)"
    )
}

struct InstallTarget {
    path: PathBuf,
    /// A one-time activation step, for shells with no autoload directory.
    activation: Option<String>,
}

/// Where each shell's completion file goes, and whether the user must wire it
/// in by hand (shells without an autoload directory).
fn completion_target(
    shell: CompletionShell,
    home: &Path,
    config_base: &Path,
    data_base: &Path,
) -> InstallTarget {
    match shell {
        // bash-completion and fish both autoload from these directories.
        CompletionShell::Bash => InstallTarget {
            path: data_base.join("bash-completion/completions/git-id"),
            activation: None,
        },
        CompletionShell::Fish => InstallTarget {
            path: config_base.join("fish/completions/git-id.fish"),
            activation: None,
        },
        // zsh autoloads from any directory on $fpath; ~/.zfunc is a common one.
        CompletionShell::Zsh => InstallTarget {
            path: home.join(".zfunc/_git-id"),
            activation: Some(
                "ensure ~/.zfunc is on your fpath: add `fpath+=(~/.zfunc)` before `compinit` in ~/.zshrc"
                    .to_string(),
            ),
        },
        // Nushell, Elvish and PowerShell have no completion autoload dir: the
        // generated file must be sourced from the shell's startup file.
        CompletionShell::Nushell => {
            let path = config_base.join("nushell/completions/git-id.nu");
            let activation = Some(format!("add `source \"{}\"` to your config.nu", path.display()));
            InstallTarget { path, activation }
        }
        CompletionShell::Elvish => {
            let path = config_base.join("elvish/lib/git-id.elv");
            let activation = Some(format!("add `eval (slurp < {})` to your rc.elv", path.display()));
            InstallTarget { path, activation }
        }
        CompletionShell::Powershell => {
            let path = config_base.join("powershell/git-id.ps1");
            let activation = Some(format!("dot-source it from your $PROFILE: `. {}`", path.display()));
            InstallTarget { path, activation }
        }
    }
}

/// Print the completion script to stdout. Needs no filesystem or HOME, so it
/// stays usable in minimal build/packaging environments.
pub fn print(shell: Option<CompletionShell>) -> Result<ExitCode> {
    let shell = resolve_shell(shell)?;
    let mut cmd = Cli::command();
    let out = &mut io::stdout();
    match shell {
        CompletionShell::Bash => generate(Shell::Bash, &mut cmd, "git-id", out),
        CompletionShell::Elvish => generate(Shell::Elvish, &mut cmd, "git-id", out),
        CompletionShell::Fish => generate(Shell::Fish, &mut cmd, "git-id", out),
        CompletionShell::Nushell => generate(Nushell, &mut cmd, "git-id", out),
        CompletionShell::Powershell => generate(Shell::PowerShell, &mut cmd, "git-id", out),
        CompletionShell::Zsh => generate(Shell::Zsh, &mut cmd, "git-id", out),
    }
    Ok(ExitCode::SUCCESS)
}

/// The filenames an executable named `name` can have on this platform, in the
/// order to probe them. Pure and `cfg!(windows)`-selected so both branches are
/// compiled and unit-tested on every host (see the cross-platform pattern in
/// paths.rs); on Unix it is just `[name]`.
fn candidate_filenames(name: &str) -> Vec<String> {
    if cfg!(windows) {
        candidate_filenames_windows(name)
    } else {
        vec![name.to_string()]
    }
}

/// Windows variant: the `.exe` form first (what shells actually ship as), then
/// the bare name. Always compiled so it is testable from Unix CI too.
fn candidate_filenames_windows(name: &str) -> Vec<String> {
    vec![format!("{name}.exe"), name.to_string()]
}

/// True if `name` resolves to a file on the current PATH (symlinks followed, so
/// Homebrew-style shims count).
fn is_in_path(name: &str) -> bool {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path_var).any(|dir| {
        candidate_filenames(name)
            .iter()
            .any(|f| dir.join(f).is_file())
    })
}

/// The shell -> binary-name table, most common first. Several shells share a
/// stable detection order so the output is deterministic.
const SHELL_BINARIES: &[(&[&str], CompletionShell)] = &[
    (&["bash"], CompletionShell::Bash),
    (&["zsh"], CompletionShell::Zsh),
    (&["fish"], CompletionShell::Fish),
    (&["nu"], CompletionShell::Nushell),
    (&["elvish"], CompletionShell::Elvish),
    (&["pwsh", "powershell"], CompletionShell::Powershell),
];

/// Shells whose executable `is_available` reports present, in `SHELL_BINARIES`
/// order. `is_available` is injectable so the logic is unit-testable without
/// touching the real PATH.
fn detect_installed_shells_with(is_available: impl Fn(&str) -> bool) -> Vec<CompletionShell> {
    SHELL_BINARIES
        .iter()
        .filter(|(bins, _)| bins.iter().any(|b| is_available(b)))
        .map(|(_, shell)| *shell)
        .collect()
}

/// All shells with a known executable on the real PATH.
fn detect_installed_shells() -> Vec<CompletionShell> {
    detect_installed_shells_with(is_in_path)
}

/// Write one shell's completion script, skipping the write when it is already
/// up to date so re-runs (and `init`) stay quiet but upgrades still refresh.
fn install_one(env: &Env, shell: CompletionShell) -> Result<()> {
    let target = completion_target(shell, &env.home, &env.config_base, &env.data_base());
    let wrote = store::write_if_changed(&target.path, &render(shell))?;
    let pretty = display_pretty(&target.path.to_string_lossy(), &env.home);
    if !wrote {
        println!(
            "{} completions already up to date ({pretty})",
            shell_name(shell)
        );
        return Ok(());
    }
    println!("Installed {} completions to {pretty}", shell_name(shell));
    match &target.activation {
        Some(step) => println!("To activate them, {step}, then restart your shell."),
        None => println!("Restart your shell (or open a new session) to use them."),
    }
    Ok(())
}

/// Install completions for each of `shells`, continuing past a per-shell error
/// (reported on stderr). Returns how many succeeded.
fn install_all(env: &Env, shells: &[CompletionShell]) -> usize {
    let mut ok = 0;
    for &shell in shells {
        match install_one(env, shell) {
            Ok(()) => ok += 1,
            Err(err) => eprintln!(
                "warning: could not install {} completions: {err:#}",
                shell_name(shell)
            ),
        }
    }
    ok
}

/// Best-effort install for every shell detected on PATH; returns the count
/// installed. Used by `init` (which must never fail on completions).
pub fn install_detected(env: &Env) -> usize {
    install_all(env, &detect_installed_shells())
}

/// Install completions: for `shell` when given, only the current shell when
/// `current`, otherwise for every shell found on PATH (falling back to the
/// running shell when PATH yields nothing).
pub fn install(env: &Env, shell: Option<CompletionShell>, current: bool) -> Result<ExitCode> {
    match shell {
        Some(shell) => install_one(env, shell)?,
        None if current => install_one(env, resolve_shell(None)?)?,
        None => {
            let shells = detect_installed_shells();
            if shells.is_empty() {
                // Nothing detected on PATH (unusual): still do something useful
                // by targeting the running shell, erroring only if that fails.
                install_one(env, resolve_shell(None)?)?;
            } else {
                install_all(env, &shells);
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Whether a shell's completion file is present and current. Carries the path
/// and whether the shell needs a manual activation line (no autoload dir).
pub struct CompletionState {
    pub shell: CompletionShell,
    pub path: PathBuf,
    pub status: CompletionStatus,
    pub needs_activation: bool,
}

pub enum CompletionStatus {
    Installed,
    Stale,
    Missing,
}

/// For each shell detected on PATH, whether git-id's completion file is in
/// place and matches the current binary. Note this only inspects the file
/// git-id writes — it cannot tell whether the user wired up the activation line
/// for shells that need one.
pub fn completion_status(env: &Env) -> Vec<CompletionState> {
    detect_installed_shells()
        .into_iter()
        .map(|shell| {
            let target = completion_target(shell, &env.home, &env.config_base, &env.data_base());
            let status = match std::fs::read_to_string(&target.path) {
                Ok(existing) if existing == render(shell) => CompletionStatus::Installed,
                Ok(_) => CompletionStatus::Stale,
                Err(_) => CompletionStatus::Missing,
            };
            CompletionState {
                shell,
                path: target.path,
                status,
                needs_activation: target.activation.is_some(),
            }
        })
        .collect()
}

/// The human label for a shell (`zsh`, `nushell`, …), for messages.
pub fn shell_display_name(shell: CompletionShell) -> String {
    shell_name(shell)
}

pub fn run(env: &Env, args: &CompletionsArgs) -> Result<ExitCode> {
    match &args.action {
        Some(CompletionsAction::Install { shell, current }) => install(env, *shell, *current),
        None => print(args.shell),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_running_shell_prefers_version_vars_over_shell() {
        // NU_VERSION in env means Nushell is the running shell, even when
        // $SHELL points at zsh (the typical case when nu is launched
        // interactively from a zsh login session).
        let env = |v: &str| match v {
            "NU_VERSION" => Some("0.99.0".into()),
            "SHELL" => Some("/bin/zsh".into()),
            _ => None,
        };
        assert_eq!(detect_running_shell(env), Some(CompletionShell::Nushell));

        let env = |v: &str| match v {
            "FISH_VERSION" => Some("3.7.0".into()),
            "SHELL" => Some("/bin/zsh".into()),
            _ => None,
        };
        assert_eq!(detect_running_shell(env), Some(CompletionShell::Fish));

        // Without version vars, falls back to $SHELL.
        let env = |v: &str| match v {
            "SHELL" => Some("/bin/zsh".into()),
            _ => None,
        };
        assert_eq!(detect_running_shell(env), Some(CompletionShell::Zsh));
    }

    #[test]
    fn detect_installed_shells_filters_and_orders() {
        // Only zsh and nu "available" -> returned in SHELL_BINARIES order.
        assert_eq!(
            detect_installed_shells_with(|b| ["nu", "zsh"].contains(&b)),
            vec![CompletionShell::Zsh, CompletionShell::Nushell]
        );
        // The pwsh/powershell alias: either binary maps to Powershell.
        assert_eq!(
            detect_installed_shells_with(|b| b == "powershell"),
            vec![CompletionShell::Powershell]
        );
        assert_eq!(
            detect_installed_shells_with(|b| b == "pwsh"),
            vec![CompletionShell::Powershell]
        );
        // Nothing available -> empty.
        assert_eq!(detect_installed_shells_with(|_| false), vec![]);
    }

    #[test]
    fn candidate_filenames_handles_windows_exe() {
        assert_eq!(
            candidate_filenames_windows("pwsh"),
            vec!["pwsh.exe", "pwsh"]
        );
        // The selected `candidate_filenames` is the identity-ish list on Unix.
        #[cfg(not(windows))]
        assert_eq!(candidate_filenames("bash"), vec!["bash"]);
        #[cfg(windows)]
        assert_eq!(candidate_filenames("bash"), vec!["bash.exe", "bash"]);
    }

    #[test]
    fn detect_shell_from_executable_path() {
        assert_eq!(detect_shell(Some("/bin/bash")), Some(CompletionShell::Bash));
        assert_eq!(detect_shell(Some("/bin/zsh")), Some(CompletionShell::Zsh));
        assert_eq!(
            detect_shell(Some("/usr/local/bin/fish")),
            Some(CompletionShell::Fish)
        );
        assert_eq!(
            detect_shell(Some("/opt/homebrew/bin/nu")),
            Some(CompletionShell::Nushell)
        );
        assert_eq!(
            detect_shell(Some("C:\\Program Files\\PowerShell\\7\\pwsh.exe")),
            Some(CompletionShell::Powershell)
        );
        assert_eq!(detect_shell(Some("/usr/bin/tcsh")), None);
        assert_eq!(detect_shell(None), None);
    }

    #[test]
    fn targets_use_xdg_dirs_and_flag_manual_shells() {
        let home = Path::new("/home/jane");
        let cfg = Path::new("/home/jane/.config");
        let data = Path::new("/home/jane/.local/share");

        let bash = completion_target(CompletionShell::Bash, home, cfg, data);
        assert_eq!(
            bash.path,
            PathBuf::from("/home/jane/.local/share/bash-completion/completions/git-id")
        );
        assert!(bash.activation.is_none());

        let fish = completion_target(CompletionShell::Fish, home, cfg, data);
        assert_eq!(
            fish.path,
            PathBuf::from("/home/jane/.config/fish/completions/git-id.fish")
        );
        assert!(fish.activation.is_none());

        let zsh = completion_target(CompletionShell::Zsh, home, cfg, data);
        assert_eq!(zsh.path, PathBuf::from("/home/jane/.zfunc/_git-id"));
        assert!(zsh.activation.is_some());

        let nu = completion_target(CompletionShell::Nushell, home, cfg, data);
        assert_eq!(
            nu.path,
            PathBuf::from("/home/jane/.config/nushell/completions/git-id.nu")
        );
        assert!(nu.activation.unwrap().contains("config.nu"));
    }

    #[test]
    fn render_emits_a_script_naming_the_binary() {
        assert!(render(CompletionShell::Bash).contains("git-id"));
        assert!(render(CompletionShell::Nushell).contains("git-id"));
    }
}
